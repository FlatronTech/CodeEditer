using CodeEditer.Services;
using CodeEditer.Utils;
using ScintillaNET;
using System;
using System.Drawing;
using System.IO;
using System.Text;
using System.Windows.Forms;

namespace CodeEditer
{
    public sealed partial class MainForm : Form
    {
        private readonly TreeView explorerTree = new TreeView();

        private MenuStrip menuStrip;
        private StatusStrip statusStrip;
        private ToolStripStatusLabel positionLabel;
        private ToolStripStatusLabel languageLabel;
        private ToolStripStatusLabel serverLabel;
        private SplitContainer splitContainer;
        private TabService tabService;
        private ExplorerService explorerService;
        private FileIconService iconService;
        private RpcService rpcService;
        private LiveServerService liveServer;

        private ToolStripMenuItem toolsMenu;
        private ToolStripMenuItem startServerItem;
        private ToolStripMenuItem stopServerItem;
        private ToolStripMenuItem closeFolderItem;

        private string workspacePath;

        public MainForm() : this(new string[0]) { }

        public MainForm(string[] args)
        {
            InitializeComponent();
            BuildUserInterface();
            InitializeServices();
            LoadSession();

            // Open files passed via "Open With..." or command line
            if (args != null)
            {
                foreach (string arg in args)
                {
                    if (File.Exists(arg))
                        tabService.AddTab(arg);
                }
            }

            // If still no tabs, open an empty one
            if (tabService.TabCount == 0)
                tabService.AddTab(null);

            // Collapse explorer if no folder is open
            if (string.IsNullOrEmpty(workspacePath))
                splitContainer.Panel1Collapsed = true;

            UpdateLiveServerMenuVisibility();
        }

        // ------------------------------------------------------------------
        // UI construction
        // ------------------------------------------------------------------

        private void BuildUserInterface()
        {
            Text = "CodeEditer";
            Size = new Size(1200, 700);
            MinimumSize = new Size(640, 400);
            StartPosition = FormStartPosition.CenterScreen;
            BackColor = Color.FromArgb(30, 30, 30);

            // Load icon
            string iconPath = Path.Combine(Application.StartupPath, "icon.ico");
            if (File.Exists(iconPath))
            {
                try { Icon = new Icon(iconPath); }
                catch { }
            }

            // Menu
            menuStrip = new MenuStrip();
            menuStrip.BackColor = Color.FromArgb(45, 45, 48);
            menuStrip.ForeColor = Color.White;
            menuStrip.Renderer = new DarkMenuRenderer();

            ToolStripMenuItem fileMenu = new ToolStripMenuItem("&File");
            fileMenu.ForeColor = Color.White;
            fileMenu.DropDownItems.Add(CreateMenuItem("&New", Keys.Control | Keys.N, OnNewClick));
            fileMenu.DropDownItems.Add(CreateMenuItem("&Open File...", Keys.Control | Keys.O, OnOpenFileClick));
            fileMenu.DropDownItems.Add(CreateMenuItem("Open Fol&der...", Keys.None, OnOpenFolderClick));
            closeFolderItem = CreateMenuItem("Close Folder", Keys.None, OnCloseFolderClick);
            fileMenu.DropDownItems.Add(closeFolderItem);
            fileMenu.DropDownItems.Add(new ToolStripSeparator());
            fileMenu.DropDownItems.Add(CreateMenuItem("&Save", Keys.Control | Keys.S, OnSaveClick));
            fileMenu.DropDownItems.Add(CreateMenuItem("Save &As...", Keys.Control | Keys.Shift | Keys.S, OnSaveAsClick));
            fileMenu.DropDownItems.Add(new ToolStripSeparator());
            fileMenu.DropDownItems.Add(CreateMenuItem("E&xit", Keys.None, OnExitClick));

            ToolStripMenuItem editMenu = new ToolStripMenuItem("&Edit");
            editMenu.ForeColor = Color.White;
            editMenu.DropDownItems.Add(CreateMenuItem("Show &Autocomplete", Keys.Control | Keys.Space, OnShowAutocompleteClick));

            ToolStripMenuItem viewMenu = new ToolStripMenuItem("&View");
            viewMenu.ForeColor = Color.White;
            viewMenu.DropDownItems.Add(CreateMenuItem("Toggle &Explorer", Keys.Control | Keys.B, OnToggleExplorerClick));

            toolsMenu = new ToolStripMenuItem("&Tools");
            toolsMenu.ForeColor = Color.White;
            startServerItem = CreateMenuItem("Start Live &Server", Keys.None, OnStartServerClick);
            stopServerItem = CreateMenuItem("S&top Live Server", Keys.None, OnStopServerClick);
            toolsMenu.DropDownItems.Add(startServerItem);
            toolsMenu.DropDownItems.Add(stopServerItem);

            // Extensions is a top-level menu item
            ToolStripMenuItem extensionsMenu = new ToolStripMenuItem("E&xtensions");
            extensionsMenu.ForeColor = Color.White;
            extensionsMenu.Click += OnExtensionsClick;

            menuStrip.Items.Add(fileMenu);
            menuStrip.Items.Add(editMenu);
            menuStrip.Items.Add(viewMenu);
            menuStrip.Items.Add(toolsMenu);
            menuStrip.Items.Add(extensionsMenu);

            // Status bar
            statusStrip = new StatusStrip();
            statusStrip.BackColor = Color.FromArgb(45, 45, 48);
            statusStrip.ForeColor = Color.White;
            statusStrip.SizingGrip = false;

            positionLabel = new ToolStripStatusLabel("Ln 1, Col 1");
            positionLabel.ForeColor = Color.White;
            languageLabel = new ToolStripStatusLabel("Plain Text");
            languageLabel.ForeColor = Color.White;
            languageLabel.Spring = true;
            languageLabel.TextAlign = ContentAlignment.MiddleRight;
            serverLabel = new ToolStripStatusLabel("");
            serverLabel.ForeColor = Color.White;

            statusStrip.Items.Add(positionLabel);
            statusStrip.Items.Add(languageLabel);
            statusStrip.Items.Add(serverLabel);

            // Tab service (creates the editor host panel)
            tabService = new TabService();

            // Split container
            splitContainer = new SplitContainer();
            splitContainer.Dock = DockStyle.Fill;
            splitContainer.FixedPanel = FixedPanel.Panel1;
            splitContainer.Panel1MinSize = 160;
            splitContainer.SplitterDistance = 230;
            splitContainer.SplitterWidth = 3;
            splitContainer.BackColor = Color.FromArgb(30, 30, 30);

            // Explorer panel
            Panel explorerHeader = new Panel();
            explorerHeader.Dock = DockStyle.Top;
            explorerHeader.Height = 28;
            explorerHeader.BackColor = Color.FromArgb(37, 37, 38);

            Label explorerLabel = new Label();
            explorerLabel.Text = "EXPLORER";
            explorerLabel.ForeColor = Color.FromArgb(150, 150, 150);
            explorerLabel.Font = new Font("Segoe UI", 9f, FontStyle.Bold);
            explorerLabel.Dock = DockStyle.Fill;
            explorerLabel.TextAlign = ContentAlignment.MiddleLeft;
            explorerLabel.Padding = new Padding(10, 0, 0, 0);
            explorerHeader.Controls.Add(explorerLabel);

            explorerTree.Dock = DockStyle.Fill;
            explorerTree.BackColor = Color.FromArgb(37, 37, 38);
            explorerTree.ForeColor = Color.FromArgb(212, 212, 212);
            explorerTree.BorderStyle = BorderStyle.None;
            explorerTree.Font = new Font("Segoe UI", 9f);
            explorerTree.ShowLines = false;
            explorerTree.ShowPlusMinus = true;
            explorerTree.ShowRootLines = false;
            explorerTree.ItemHeight = 24;
            explorerTree.FullRowSelect = true;
            explorerTree.HideSelection = false;
            explorerTree.ShowNodeToolTips = true;

            splitContainer.Panel1.BackColor = Color.FromArgb(37, 37, 38);
            splitContainer.Panel1.Controls.Add(explorerTree);
            splitContainer.Panel1.Controls.Add(explorerHeader);

            splitContainer.Panel2.Controls.Add(tabService.Control);

            Controls.Add(splitContainer);
            Controls.Add(statusStrip);
            Controls.Add(menuStrip);
            MainMenuStrip = menuStrip;
        }

        private static ToolStripMenuItem CreateMenuItem(string text, Keys shortcut, EventHandler handler)
        {
            ToolStripMenuItem item = new ToolStripMenuItem(text);
            item.ForeColor = Color.White;
            if (shortcut != Keys.None)
                item.ShortcutKeys = shortcut;
            item.Click += handler;
            return item;
        }

        private void InitializeServices()
        {
            iconService = new FileIconService();
            explorerService = new ExplorerService(explorerTree, iconService);
            explorerService.FileActivated += OnExplorerFileActivated;

            // Tab events
            tabService.NewTabCreated += OnNewTabCreated;
            tabService.ActiveTabChanged += OnActiveTabChanged;

            // Live Server (always create but only use if extension installed)
            liveServer = new LiveServerService();
            liveServer.Log += OnServerLog;
        }

        private void UpdateLiveServerMenuVisibility()
        {
            bool show = ExtensionManager.IsLiveServerInstalled;
            startServerItem.Visible = show;
            stopServerItem.Visible = show;
            // Hide Tools menu entirely if nothing to show
            toolsMenu.Visible = show;
        }

        // ------------------------------------------------------------------
        // Tab event handlers
        // ------------------------------------------------------------------

        private void OnNewTabCreated(Scintilla editor)
        {
            // Configure the editor with dark theme
            EditorService.ConfigureEditor(editor);

            // Hook up UpdateUI for position tracking
            editor.UpdateUI += OnEditorUpdateUi;

            // Hook up SavePointLeft/SavePointReached for modified indicator
            editor.SavePointLeft += OnSavePointLeft;
            editor.SavePointReached += OnSavePointReached;
        }

        private void OnActiveTabChanged(int index)
        {
            if (index < 0)
            {
                Text = "CodeEditer";
                positionLabel.Text = "Ln 1, Col 1";
                languageLabel.Text = "Plain Text";
                return;
            }

            TabService.TabInfo tab = tabService.GetActiveTab();
            if (tab == null) return;

            // Apply language profile
            string fileName = tab.IsNew ? "untitled.txt" : tab.FilePath;
            LanguageProfile profile = Languages.FromFileName(fileName);
            EditorService svc = new EditorService(tab.Editor);
            svc.ApplyProfile(profile);
            languageLabel.Text = profile.Name;

            UpdateTitle();
            UpdateCursorPosition();
            UpdateRpc();
        }

        private void OnSavePointLeft(object sender, EventArgs e)
        {
            // Find which tab this editor belongs to
            int idx = FindTabForEditor(sender as Scintilla);
            if (idx >= 0)
            {
                tabService.UpdateTabTitle(idx);
                if (idx == tabService.ActiveIndex)
                    UpdateTitle();
            }
        }

        private void OnSavePointReached(object sender, EventArgs e)
        {
            int idx = FindTabForEditor(sender as Scintilla);
            if (idx >= 0)
            {
                tabService.UpdateTabTitle(idx);
                if (idx == tabService.ActiveIndex)
                    UpdateTitle();
            }
        }

        private int FindTabForEditor(Scintilla editor)
        {
            if (editor == null) return -1;
            for (int i = 0; i < tabService.TabCount; i++)
            {
                TabService.TabInfo info = tabService.GetTabAt(i);
                if (info != null && info.Editor == editor)
                    return i;
            }
            return -1;
        }

        // ------------------------------------------------------------------
        // Menu handlers
        // ------------------------------------------------------------------

        private void OnNewClick(object sender, EventArgs e)
        {
            tabService.AddTab(null);
        }

        private void OnOpenFileClick(object sender, EventArgs e)
        {
            using (OpenFileDialog dialog = new OpenFileDialog())
            {
                dialog.Filter =
                    "Supported files|*.cs;*.js;*.json;*.htm;*.html;*.css;*.txt;*.cpp;*.css;*.py;*.asm;*.c;*.bat;*.rs|All files (*.*)|*.*";
                dialog.Multiselect = true;
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    foreach (string file in dialog.FileNames)
                    {
                        OpenFileInTab(file);
                    }
                }
            }
        }

        private void OnOpenFolderClick(object sender, EventArgs e)
        {
            using (FolderBrowserDialog dialog = new FolderBrowserDialog())
            {
                dialog.Description = "Select a folder to display in the file explorer.";
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    workspacePath = dialog.SelectedPath;
                    explorerService.SetRootPath(workspacePath);
                    splitContainer.Panel1Collapsed = false;
                    UpdateRpc();
                }
            }
        }

        private void OnCloseFolderClick(object sender, EventArgs e)
        {
            workspacePath = null;
            explorerTree.Nodes.Clear();
            splitContainer.Panel1Collapsed = true;
            UpdateRpc();
        }

        private void OnSaveClick(object sender, EventArgs e)
        {
            SaveCurrentTab(false);
        }

        private void OnSaveAsClick(object sender, EventArgs e)
        {
            SaveCurrentTab(true);
        }

        private void OnShowAutocompleteClick(object sender, EventArgs e)
        {
            Scintilla editor = tabService.GetCurrentEditor();
            if (editor != null)
                editor.Focus();
        }

        private void OnToggleExplorerClick(object sender, EventArgs e)
        {
            splitContainer.Panel1Collapsed = !splitContainer.Panel1Collapsed;
        }

        private void OnExtensionsClick(object sender, EventArgs e)
        {
            using (ExtensionsForm form = new ExtensionsForm())
            {
                form.ShowDialog(this);
            }
            // After closing Extensions, update Live Server menu visibility
            UpdateLiveServerMenuVisibility();
            // If Discord RPC was just installed, start it
            if (ExtensionManager.IsDiscordRpcInstalled && rpcService == null)
            {
                rpcService = new RpcService();
                rpcService.Start();
                UpdateRpc();
            }
        }

        private void OnStartServerClick(object sender, EventArgs e)
        {
            if (!ExtensionManager.IsLiveServerInstalled)
            {
                MessageBox.Show(this, "Live Server extension is not installed.", "CodeEditer", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }

            if (liveServer.IsRunning)
            {
                MessageBox.Show(this, "Live Server is already running on port " + liveServer.Port,
                    "CodeEditer", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }

            string servePath = workspacePath;
            if (string.IsNullOrEmpty(servePath))
            {
                // Try to use directory of current file
                TabService.TabInfo tab = tabService.GetActiveTab();
                if (tab != null && !tab.IsNew && !string.IsNullOrEmpty(tab.FilePath))
                    servePath = Path.GetDirectoryName(tab.FilePath);
            }

            if (string.IsNullOrEmpty(servePath) || !Directory.Exists(servePath))
            {
                MessageBox.Show(this, "Open a folder or file first.", "CodeEditer",
                    MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }

            if (Directory.GetFiles(servePath, "*.html", SearchOption.AllDirectories).Length == 0)
            {
                MessageBox.Show(this, "No .html files found in the directory.", "CodeEditer", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }

            liveServer.Start(servePath, 5500);
            if (liveServer.IsRunning)
            {
                serverLabel.Text = "Live Server: http://localhost:" + liveServer.Port;
                try
                {
                    System.Diagnostics.Process.Start("http://localhost:" + liveServer.Port);
                }
                catch { }
            }
        }

        private void OnStopServerClick(object sender, EventArgs e)
        {
            liveServer.Stop();
            serverLabel.Text = "";
        }

        private void OnExitClick(object sender, EventArgs e)
        {
            Close();
        }

        // ------------------------------------------------------------------
        // Explorer
        // ------------------------------------------------------------------

        private void OnExplorerFileActivated(string path)
        {
            OpenFileInTab(path);
        }

        // ------------------------------------------------------------------
        // File operations
        // ------------------------------------------------------------------

        private void OpenFileInTab(string path)
        {
            tabService.AddTab(path);
        }

        private bool SaveCurrentTab(bool forceDialog)
        {
            TabService.TabInfo tab = tabService.GetActiveTab();
            if (tab == null) return false;

            if (forceDialog || tab.IsNew || string.IsNullOrEmpty(tab.FilePath))
            {
                using (SaveFileDialog dialog = new SaveFileDialog())
                {
                    dialog.Filter =
                        "C# files (*.cs)|*.cs|JavaScript files (*.js)|*.js|" +
                        "HTML files (*.html)|*.html|CSS files (*.css)|*.css|" +
                        "Text files (*.txt)|*.txt|All files (*.*)|*.*";
                    dialog.FileName = tab.IsNew ? "untitled.txt" : Path.GetFileName(tab.FilePath);

                    if (dialog.ShowDialog(this) != DialogResult.OK)
                        return false;

                    tab.FilePath = dialog.FileName;
                    tab.IsNew = false;
                }
            }

            try
            {
                File.WriteAllText(tab.FilePath, tab.Editor.Text, new UTF8Encoding(false));
                tab.Editor.SetSavePoint();
                tabService.UpdateTabTitle(tabService.ActiveIndex);
                UpdateTitle();

                // Re-apply language in case extension changed
                LanguageProfile profile = Languages.FromFileName(tab.FilePath);
                EditorService svc = new EditorService(tab.Editor);
                svc.ApplyProfile(profile);
                languageLabel.Text = profile.Name;
                return true;
            }
            catch (Exception ex)
            {
                MessageBox.Show(this,
                    "Could not save file:" + Environment.NewLine + ex.Message,
                    "CodeEditer", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return false;
            }
        }

        // ------------------------------------------------------------------
        // Status bar / title
        // ------------------------------------------------------------------

        private void OnEditorUpdateUi(object sender, UpdateUIEventArgs e)
        {
            Scintilla editor = sender as Scintilla;
            if (editor == null) return;

            // Only update if this is the active editor
            Scintilla current = tabService.GetCurrentEditor();
            if (current != editor) return;

            UpdateCursorPosition();
        }

        private void UpdateCursorPosition()
        {
            Scintilla editor = tabService.GetCurrentEditor();
            if (editor == null) return;

            int position = editor.CurrentPosition;
            int line = editor.LineFromPosition(position);
            int column = position - editor.Lines[line].Position;
            positionLabel.Text = string.Format("Ln {0}, Col {1}", line + 1, column + 1);
        }

        private void UpdateTitle()
        {
            TabService.TabInfo tab = tabService.GetActiveTab();
            if (tab == null)
            {
                Text = "CodeEditer";
                return;
            }

            string name = tab.IsNew ? "Untitled" : Path.GetFileName(tab.FilePath);
            Text = (tab.Editor.Modified ? "\u25cf " : "") + name + " - CodeEditer";
        }

        private void UpdateRpc()
        {
            if (rpcService == null) return;

            TabService.TabInfo tab = tabService.GetActiveTab();
            string fileName = null;
            if (tab != null && !tab.IsNew)
                fileName = Path.GetFileName(tab.FilePath);

            string ws = null;
            if (!string.IsNullOrEmpty(workspacePath))
                ws = Path.GetFileName(workspacePath);

            rpcService.UpdatePresence(fileName, ws);
        }

        private void OnServerLog(string message)
        {
            // Marshal to UI thread if needed
            if (InvokeRequired)
            {
                BeginInvoke(new Action<string>(OnServerLog), message);
                return;
            }
            serverLabel.Text = message;
        }

        // ------------------------------------------------------------------
        // Session
        // ------------------------------------------------------------------

        private void SaveSession()
        {
            try
            {
                var files = new System.Collections.Generic.List<string>();
                for (int i = 0; i < tabService.TabCount; i++)
                {
                    var tab = tabService.GetTabAt(i);
                    if (!tab.IsNew && !string.IsNullOrEmpty(tab.FilePath))
                        files.Add(tab.FilePath.Replace("\\", "\\\\").Replace("\"", "\\\""));
                }
                string ws = string.IsNullOrEmpty(workspacePath) ? "" : workspacePath.Replace("\\", "\\\\").Replace("\"", "\\\"");

                string json = "{\n";
                json += "  \"workspacePath\": \"" + ws + "\",\n";
                json += "  \"liveServer\": " + ExtensionManager.IsLiveServerInstalled.ToString().ToLower() + ",\n";
                json += "  \"discordRpc\": " + ExtensionManager.IsDiscordRpcInstalled.ToString().ToLower() + ",\n";
                json += "  \"files\": [\n";
                for (int i = 0; i < files.Count; i++)
                {
                    json += "    \"" + files[i] + "\"" + (i < files.Count - 1 ? "," : "") + "\n";
                }
                json += "  ]\n}";

                File.WriteAllText(Path.Combine(Application.StartupPath, "session.json"), json);
            }
            catch { }
        }

        private void LoadSession()
        {
            try
            {
                string path = Path.Combine(Application.StartupPath, "session.json");
                if (!File.Exists(path)) return;

                string json = File.ReadAllText(path);

                // Load extension states
                var matchLs = System.Text.RegularExpressions.Regex.Match(json, "\"liveServer\":\\s*(true|false)");
                if (matchLs.Success) ExtensionManager.IsLiveServerInstalled = matchLs.Groups[1].Value == "true";

                var matchRpc = System.Text.RegularExpressions.Regex.Match(json, "\"discordRpc\":\\s*(true|false)");
                if (matchRpc.Success) ExtensionManager.IsDiscordRpcInstalled = matchRpc.Groups[1].Value == "true";

                // Start Discord RPC if installed
                if (ExtensionManager.IsDiscordRpcInstalled)
                {
                    rpcService = new RpcService();
                    rpcService.Start();
                }

                // Load workspace
                var matchWs = System.Text.RegularExpressions.Regex.Match(json, "\"workspacePath\":\\s*\"([^\"]*)\"");
                if (matchWs.Success)
                {
                    string ws = matchWs.Groups[1].Value.Replace("\\\\", "\\");
                    if (!string.IsNullOrEmpty(ws) && Directory.Exists(ws))
                    {
                        workspacePath = ws;
                        explorerService.SetRootPath(workspacePath);
                        splitContainer.Panel1Collapsed = false;
                    }
                }

                // Load files
                var matchFiles = System.Text.RegularExpressions.Regex.Match(json, "\"files\":\\s*\\[(.*?)\\]", System.Text.RegularExpressions.RegexOptions.Singleline);
                if (matchFiles.Success)
                {
                    string filesStr = matchFiles.Groups[1].Value;
                    var fileMatches = System.Text.RegularExpressions.Regex.Matches(filesStr, "\"([^\"]+)\"");
                    foreach (System.Text.RegularExpressions.Match fm in fileMatches)
                    {
                        string f = fm.Groups[1].Value.Replace("\\\\", "\\");
                        if (File.Exists(f))
                        {
                            tabService.AddTab(f);
                        }
                    }
                }
            }
            catch { }
        }

        // ------------------------------------------------------------------
        // Lifetime
        // ------------------------------------------------------------------

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            if (tabService.HasUnsavedChanges())
            {
                DialogResult result = MessageBox.Show(this,
                    "You have unsaved changes. Close anyway?", "CodeEditer",
                    MessageBoxButtons.YesNo, MessageBoxIcon.Question);
                if (result == DialogResult.No)
                {
                    e.Cancel = true;
                    return;
                }
            }
            base.OnFormClosing(e);
        }

        protected override void OnFormClosed(FormClosedEventArgs e)
        {
            SaveSession();
            if (rpcService != null) rpcService.Dispose();
            if (liveServer != null) liveServer.Dispose();
            if (iconService != null) iconService.Dispose();
            base.OnFormClosed(e);
        }
    }

    // ------------------------------------------------------------------
    // Dark menu renderer
    // ------------------------------------------------------------------

    public class DarkMenuRenderer : ToolStripProfessionalRenderer
    {
        public DarkMenuRenderer() : base(new DarkColorTable()) { }
    }

    public class DarkColorTable : ProfessionalColorTable
    {
        public override Color MenuItemSelected { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuBorder { get { return Color.FromArgb(45, 45, 48); } }
        public override Color MenuItemBorder { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuItemSelectedGradientBegin { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuItemSelectedGradientEnd { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuItemPressedGradientBegin { get { return Color.FromArgb(50, 50, 52); } }
        public override Color MenuItemPressedGradientEnd { get { return Color.FromArgb(50, 50, 52); } }
        public override Color MenuStripGradientBegin { get { return Color.FromArgb(45, 45, 48); } }
        public override Color MenuStripGradientEnd { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ToolStripDropDownBackground { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ImageMarginGradientBegin { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ImageMarginGradientMiddle { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ImageMarginGradientEnd { get { return Color.FromArgb(45, 45, 48); } }
        public override Color SeparatorDark { get { return Color.FromArgb(62, 62, 64); } }
        public override Color SeparatorLight { get { return Color.FromArgb(62, 62, 64); } }
    }
}