using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading;

namespace CodeEditer.Services
{
    /// <summary>
    /// Discord Rich Presence via raw Named Pipe IPC (no external libraries).
    /// Connects to Discord client pipe, sends handshake and presence updates.
    /// </summary>
    public sealed class RpcService : IDisposable
    {
        private const string ClientId = "1545533503519596594";
        private const int HandshakeOpcode = 0;
        private const int FrameOpcode = 1;

        private NamedPipeClientStream pipe;
        private Thread connectThread;
        private volatile bool disposed;
        private volatile bool connected;
        private readonly object writeLock = new object();
        private long startTimestamp;

        public RpcService()
        {
            startTimestamp = (long)(DateTime.UtcNow.Subtract(new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc)).TotalSeconds);
        }

        public void Start()
        {
            if (connectThread != null)
                return;
            connectThread = new Thread(ConnectLoop);
            connectThread.IsBackground = true;
            connectThread.Name = "DiscordRPC";
            connectThread.Start();
        }

        public void UpdatePresence(string fileName, string workspace)
        {
            if (!connected || disposed)
                return;

            string details = string.IsNullOrEmpty(fileName) ? "Doing nothing" : "Editing " + fileName;
            string state = string.IsNullOrEmpty(workspace) ? "No workspace" : workspace;

            string payload = "{" +
                "\"cmd\":\"SET_ACTIVITY\"," +
                "\"args\":{" +
                    "\"pid\":" + System.Diagnostics.Process.GetCurrentProcess().Id + "," +
                    "\"activity\":{" +
                        "\"details\":\"" + EscapeJson(details) + "\"," +
                        "\"state\":\"" + EscapeJson(state) + "\"," +
                        "\"timestamps\":{\"start\":" + startTimestamp + "}," +
                        "\"assets\":{" +
                            "\"large_image\":\"logo\"," +
                            "\"large_text\":\"CodeEditer\"" +
                        "}" +
                    "}" +
                "}," +
                "\"nonce\":\"" + Guid.NewGuid().ToString() + "\"" +
            "}";

            SendFrame(FrameOpcode, payload);
        }

        public void Dispose()
        {
            disposed = true;
            connected = false;
            try
            {
                if (pipe != null)
                {
                    pipe.Close();
                    pipe.Dispose();
                }
            }
            catch { }
        }

        private void ConnectLoop()
        {
            while (!disposed)
            {
                for (int i = 0; i < 10; i++)
                {
                    if (disposed) return;
                    try
                    {
                        string pipeName = "discord-ipc-" + i;
                        pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
                        pipe.Connect(1000);

                        if (pipe.IsConnected)
                        {
                            // Send handshake
                            string handshake = "{\"v\":1,\"client_id\":\"" + ClientId + "\"}";
                            SendFrame(HandshakeOpcode, handshake);

                            // Read response
                            byte[] header = new byte[8];
                            int bytesRead = pipe.Read(header, 0, 8);
                            if (bytesRead == 8)
                            {
                                int dataLen = BitConverter.ToInt32(header, 4);
                                byte[] data = new byte[dataLen];
                                pipe.Read(data, 0, dataLen);
                                // If we got here without exception, we're connected
                                connected = true;
                                UpdatePresence(null, null);

                                // Keep alive: read in loop
                                while (!disposed && pipe.IsConnected)
                                {
                                    try
                                    {
                                        byte[] readHeader = new byte[8];
                                        int read = pipe.Read(readHeader, 0, 8);
                                        if (read == 0) break;
                                        int len = BitConverter.ToInt32(readHeader, 4);
                                        byte[] readData = new byte[len];
                                        pipe.Read(readData, 0, len);
                                    }
                                    catch { break; }
                                }
                            }
                        }
                    }
                    catch { }
                    finally
                    {
                        connected = false;
                        try { if (pipe != null) pipe.Dispose(); }
                        catch { }
                        pipe = null;
                    }
                }

                // Wait before retry
                if (!disposed)
                    Thread.Sleep(15000);
            }
        }

        private void SendFrame(int opcode, string payload)
        {
            if (pipe == null || !pipe.IsConnected)
                return;

            lock (writeLock)
            {
                try
                {
                    byte[] data = Encoding.UTF8.GetBytes(payload);
                    byte[] header = new byte[8];
                    Array.Copy(BitConverter.GetBytes(opcode), 0, header, 0, 4);
                    Array.Copy(BitConverter.GetBytes(data.Length), 0, header, 4, 4);
                    pipe.Write(header, 0, header.Length);
                    pipe.Write(data, 0, data.Length);
                    pipe.Flush();
                }
                catch { }
            }
        }

        private static string EscapeJson(string s)
        {
            if (string.IsNullOrEmpty(s)) return s;
            StringBuilder sb = new StringBuilder(s.Length);
            for (int i = 0; i < s.Length; i++)
            {
                char c = s[i];
                switch (c)
                {
                    case '"': sb.Append("\\\""  ); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default: sb.Append(c); break;
                }
            }
            return sb.ToString();
        }
    }
}
