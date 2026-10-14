using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Text;
using System.Threading;

namespace CodeEditer.Services
{
    /// <summary>
    /// Lightweight HTTP development server with auto-reload.
    /// Serves static files from a workspace directory and injects
    /// a polling script into HTML pages for live reload.
    /// </summary>
    public sealed class LiveServerService : IDisposable
    {
        private HttpListener listener;
        private Thread serverThread;
        private FileSystemWatcher watcher;
        private volatile bool disposed;
        private volatile bool running;
        private string rootPath;
        private int port;
        private long changeVersion;

        private static readonly Dictionary<string, string> MimeTypes = BuildMimeTypes();

        public event Action<string> Log;

        public int Port { get { return port; } }
        public bool IsRunning { get { return running; } }

        public void Start(string workspacePath, int preferredPort)
        {
            if (running) return;
            if (string.IsNullOrEmpty(workspacePath) || !Directory.Exists(workspacePath))
                return;

            rootPath = workspacePath;
            port = preferredPort;

            // Try to find an available port
            for (int attempt = 0; attempt < 10; attempt++)
            {
                try
                {
                    listener = new HttpListener();
                    listener.Prefixes.Add("http://localhost:" + port + "/");
                    listener.Start();
                    break;
                }
                catch
                {
                    port++;
                    if (listener != null)
                    {
                        try { listener.Close(); } catch { }
                    }
                    listener = null;
                }
            }

            if (listener == null || !listener.IsListening)
            {
                RaiseLog("Failed to start server on any port.");
                return;
            }

            running = true;
            changeVersion = 0;

            // File watcher for auto-reload
            watcher = new FileSystemWatcher(rootPath);
            watcher.IncludeSubdirectories = true;
            watcher.NotifyFilter = NotifyFilters.LastWrite | NotifyFilters.FileName | NotifyFilters.DirectoryName;
            watcher.Changed += OnFileChanged;
            watcher.Created += OnFileChanged;
            watcher.Deleted += OnFileChanged;
            watcher.Renamed += OnFileRenamed;
            watcher.EnableRaisingEvents = true;

            serverThread = new Thread(ServerLoop);
            serverThread.IsBackground = true;
            serverThread.Name = "LiveServer";
            serverThread.Start();

            RaiseLog("Live Server started at http://localhost:" + port + "/");
        }

        public void Stop()
        {
            running = false;
            if (watcher != null)
            {
                watcher.EnableRaisingEvents = false;
                watcher.Dispose();
                watcher = null;
            }
            if (listener != null)
            {
                try { listener.Stop(); } catch { }
                try { listener.Close(); } catch { }
                listener = null;
            }
            RaiseLog("Live Server stopped.");
        }

        public void Dispose()
        {
            disposed = true;
            Stop();
        }

        private void ServerLoop()
        {
            while (running && !disposed)
            {
                try
                {
                    HttpListenerContext ctx = listener.GetContext();
                    ThreadPool.QueueUserWorkItem(HandleRequest, ctx);
                }
                catch (HttpListenerException)
                {
                    if (!running) break;
                }
                catch (ObjectDisposedException)
                {
                    break;
                }
                catch { }
            }
        }

        private void HandleRequest(object state)
        {
            HttpListenerContext ctx = (HttpListenerContext)state;
            try
            {
                string rawUrl = ctx.Request.Url.AbsolutePath;

                // Change-poll endpoint for live reload
                if (rawUrl == "/__livereload__")
                {
                    string versionParam = ctx.Request.QueryString["v"];
                    long clientVersion;
                    long.TryParse(versionParam, out clientVersion);

                    string json = "{\"version\":" + Interlocked.Read(ref changeVersion) + "," +
                                  "\"changed\":" + (clientVersion < Interlocked.Read(ref changeVersion) ? "true" : "false") + "}";

                    byte[] jsonBytes = Encoding.UTF8.GetBytes(json);
                    ctx.Response.ContentType = "application/json";
                    ctx.Response.ContentLength64 = jsonBytes.Length;
                    ctx.Response.OutputStream.Write(jsonBytes, 0, jsonBytes.Length);
                    ctx.Response.OutputStream.Close();
                    return;
                }

                // Serve static files
                string urlPath = Uri.UnescapeDataString(rawUrl).TrimStart('/');
                if (string.IsNullOrEmpty(urlPath))
                    urlPath = "index.html";

                string filePath = Path.Combine(rootPath, urlPath.Replace('/', '\\'));

                // Security: ensure file is within root
                string fullRoot = Path.GetFullPath(rootPath);
                string fullFile = Path.GetFullPath(filePath);
                if (!fullFile.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
                {
                    SendError(ctx, 403, "Forbidden");
                    return;
                }

                if (!File.Exists(filePath))
                {
                    // Try index.html in directory
                    if (Directory.Exists(filePath))
                    {
                        string indexPath = Path.Combine(filePath, "index.html");
                        if (File.Exists(indexPath))
                            filePath = indexPath;
                        else
                        {
                            SendError(ctx, 404, "Not Found");
                            return;
                        }
                    }
                    else
                    {
                        SendError(ctx, 404, "Not Found");
                        return;
                    }
                }

                byte[] fileBytes = File.ReadAllBytes(filePath);
                string ext = Path.GetExtension(filePath).ToLowerInvariant();
                string mimeType;
                if (!MimeTypes.TryGetValue(ext, out mimeType))
                    mimeType = "application/octet-stream";

                // Inject live reload script into HTML
                if (ext == ".html" || ext == ".htm")
                {
                    string html = Encoding.UTF8.GetString(fileBytes);
                    string reloadScript =
                        "\n<script>(function(){var v=0;setInterval(function(){" +
                        "var x=new XMLHttpRequest();x.open('GET','/__livereload__?v='+v,true);" +
                        "x.onload=function(){if(x.status===200){var r=JSON.parse(x.responseText);" +
                        "if(r.changed){location.reload();}v=r.version;}};x.send();" +
                        "},1000);})()</script>\n";

                    int bodyClose = html.LastIndexOf("</body>", StringComparison.OrdinalIgnoreCase);
                    if (bodyClose >= 0)
                        html = html.Substring(0, bodyClose) + reloadScript + html.Substring(bodyClose);
                    else
                        html = html + reloadScript;

                    fileBytes = Encoding.UTF8.GetBytes(html);
                }

                ctx.Response.ContentType = mimeType;
                ctx.Response.ContentLength64 = fileBytes.Length;
                ctx.Response.OutputStream.Write(fileBytes, 0, fileBytes.Length);
                ctx.Response.OutputStream.Close();
            }
            catch
            {
                try { SendError(ctx, 500, "Internal Server Error"); }
                catch { }
            }
        }

        private void SendError(HttpListenerContext ctx, int code, string message)
        {
            try
            {
                ctx.Response.StatusCode = code;
                byte[] body = Encoding.UTF8.GetBytes("<html><body><h1>" + code + " " + message + "</h1></body></html>");
                ctx.Response.ContentType = "text/html";
                ctx.Response.ContentLength64 = body.Length;
                ctx.Response.OutputStream.Write(body, 0, body.Length);
                ctx.Response.OutputStream.Close();
            }
            catch { }
        }

        private void OnFileChanged(object sender, FileSystemEventArgs e)
        {
            Interlocked.Increment(ref changeVersion);
        }

        private void OnFileRenamed(object sender, RenamedEventArgs e)
        {
            Interlocked.Increment(ref changeVersion);
        }

        private void RaiseLog(string message)
        {
            Action<string> handler = Log;
            if (handler != null)
                handler(message);
        }

        private static Dictionary<string, string> BuildMimeTypes()
        {
            Dictionary<string, string> map = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            map[".html"] = "text/html; charset=utf-8";
            map[".htm"] = "text/html; charset=utf-8";
            map[".css"] = "text/css; charset=utf-8";
            map[".js"] = "application/javascript; charset=utf-8";
            map[".json"] = "application/json; charset=utf-8";
            map[".png"] = "image/png";
            map[".jpg"] = "image/jpeg";
            map[".jpeg"] = "image/jpeg";
            map[".gif"] = "image/gif";
            map[".svg"] = "image/svg+xml";
            map[".ico"] = "image/x-icon";
            map[".txt"] = "text/plain; charset=utf-8";
            map[".xml"] = "text/xml; charset=utf-8";
            map[".woff"] = "font/woff";
            map[".woff2"] = "font/woff2";
            map[".ttf"] = "font/ttf";
            map[".eot"] = "application/vnd.ms-fontobject";
            map[".mp4"] = "video/mp4";
            map[".webm"] = "video/webm";
            map[".mp3"] = "audio/mpeg";
            map[".wav"] = "audio/wav";
            map[".pdf"] = "application/pdf";
            map[".zip"] = "application/zip";
            map[".wasm"] = "application/wasm";
            return map;
        }
    }
}
