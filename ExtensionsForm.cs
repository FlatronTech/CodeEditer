using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace CodeEditer
{
    public static class ExtensionManager
    {
        public static bool IsLiveServerInstalled { get; set; }
        public static bool IsDiscordRpcInstalled { get; set; }
    }

    public class ExtensionsForm : Form
    {
        private Panel listPanel;
        private Panel detailsPanel;
        private Label titleLabel;
        private Label descLabel;
        private Label pubLabel;
        private PictureBox verifiedIcon;
        private Button actionButton;
        private Button backButton;
        private PictureBox warningIcon;
        private Label warningLabel;

        private string currentExtension;

        public ExtensionsForm()
        {
            Text = "Extensions";
            Size = new Size(460, 360);
            StartPosition = FormStartPosition.CenterParent;
            BackColor = Color.FromArgb(30, 30, 30);
            ForeColor = Color.White;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;
            ShowIcon = false;

            listPanel = new Panel();
            listPanel.Dock = DockStyle.Fill;

            detailsPanel = new Panel();
            detailsPanel.Dock = DockStyle.Fill;
            detailsPanel.Visible = false;

            Controls.Add(detailsPanel);
            Controls.Add(listPanel);

            // List panel setup
            Button item1 = new Button();
            item1.Text = "Live Server";
            item1.Location = new Point(20, 20);
            item1.Size = new Size(360, 40);
            item1.FlatStyle = FlatStyle.Flat;
            item1.TextAlign = ContentAlignment.MiddleLeft;
            item1.ForeColor = Color.White;
            item1.BackColor = Color.FromArgb(51, 51, 51);
            item1.FlatAppearance.BorderColor = Color.FromArgb(70, 70, 70);
            item1.Click += delegate { ShowDetails("Live Server", "Launch a local development server with live reload feature for HTML, CSS & JavaScript.", "FlatronTech"); };

            Button item2 = new Button();
            item2.Text = "Discord RPC";
            item2.Location = new Point(20, 70);
            item2.Size = new Size(360, 40);
            item2.FlatStyle = FlatStyle.Flat;
            item2.TextAlign = ContentAlignment.MiddleLeft;
            item2.ForeColor = Color.White;
            item2.BackColor = Color.FromArgb(51, 51, 51);
            item2.FlatAppearance.BorderColor = Color.FromArgb(70, 70, 70);
            item2.Click += delegate { ShowDetails("Discord RPC", "Show your current editing activity as Rich Presence on Discord.", "FlatronTech"); };

            listPanel.Controls.Add(item1);
            listPanel.Controls.Add(item2);

            // Details panel setup
            backButton = new Button();
            backButton.Text = "← Back";
            backButton.Location = new Point(10, 10);
            backButton.Size = new Size(80, 28);
            backButton.FlatStyle = FlatStyle.Flat;
            backButton.ForeColor = Color.White;
            backButton.BackColor = Color.FromArgb(51, 51, 51);
            backButton.FlatAppearance.BorderColor = Color.FromArgb(70, 70, 70);
            backButton.Click += delegate { detailsPanel.Visible = false; listPanel.Visible = true; };

            titleLabel = new Label();
            titleLabel.Location = new Point(10, 50);
            titleLabel.Font = new Font("Segoe UI", 14, FontStyle.Bold);
            titleLabel.AutoSize = true;

            // Publisher label + verified icon placed next to publisher
            pubLabel = new Label();
            pubLabel.Location = new Point(10, 85);
            pubLabel.ForeColor = Color.LightGray;
            pubLabel.Font = new Font("Segoe UI", 9f);
            pubLabel.AutoSize = true;

            verifiedIcon = new PictureBox();
            verifiedIcon.Size = new Size(16, 16);
            verifiedIcon.SizeMode = PictureBoxSizeMode.StretchImage;
            verifiedIcon.BackColor = Color.Transparent;
            string vPath = Path.Combine(Application.StartupPath, "icons", "verified.png");
            if (File.Exists(vPath))
            {
                try { verifiedIcon.Image = Image.FromFile(vPath); }
                catch { }
            }

            descLabel = new Label();
            descLabel.Location = new Point(10, 115);
            descLabel.Size = new Size(380, 60);
            descLabel.ForeColor = Color.FromArgb(200, 200, 200);
            descLabel.Font = new Font("Segoe UI", 9f);

            actionButton = new Button();
            actionButton.Location = new Point(10, 220);
            actionButton.Size = new Size(160, 36);
            actionButton.BackColor = Color.FromArgb(14, 99, 156);
            actionButton.FlatStyle = FlatStyle.Flat;
            actionButton.ForeColor = Color.White;
            actionButton.FlatAppearance.BorderColor = Color.FromArgb(14, 99, 156);
            actionButton.Font = new Font("Segoe UI", 9f, FontStyle.Bold);
            actionButton.Click += OnActionClick;

            warningIcon = new PictureBox();
            warningIcon.Size = new Size(16, 16);
            warningIcon.SizeMode = PictureBoxSizeMode.StretchImage;
            warningIcon.BackColor = Color.Transparent;
            warningIcon.Location = new Point(10, 180);
            string wPath = Path.Combine(Application.StartupPath, "icons", "warning.png");
            if (File.Exists(wPath))
            {
                try { warningIcon.Image = Image.FromFile(wPath); }
                catch { }
            }
            warningIcon.Visible = false;

            warningLabel = new Label();
            warningLabel.Location = new Point(30, 180);
            warningLabel.Size = new Size(360, 30);
            warningLabel.ForeColor = Color.Orange;
            warningLabel.Font = new Font("Segoe UI", 9f);
            warningLabel.Text = "This extension isn't going to work on your system.";
            warningLabel.Visible = false;

            detailsPanel.Controls.Add(backButton);
            detailsPanel.Controls.Add(titleLabel);
            detailsPanel.Controls.Add(pubLabel);
            detailsPanel.Controls.Add(verifiedIcon);
            detailsPanel.Controls.Add(descLabel);
            detailsPanel.Controls.Add(warningIcon);
            detailsPanel.Controls.Add(warningLabel);
            detailsPanel.Controls.Add(actionButton);
        }

        private void ShowDetails(string name, string desc, string pub)
        {
            currentExtension = name;
            titleLabel.Text = name;

            pubLabel.Text = pub;

            // Place verified icon next to publisher name
            int pubWidth = TextRenderer.MeasureText(pub, pubLabel.Font).Width;
            verifiedIcon.Location = new Point(pubLabel.Left + pubWidth + 2, pubLabel.Top + 1);

            descLabel.Text = desc;

            // OS check for Discord RPC
            bool isUnsupported = false;
            if (name == "Discord RPC")
            {
                try
                {
                    using (var key = Microsoft.Win32.Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion"))
                    {
                        if (key != null)
                        {
                            object majorObj = key.GetValue("CurrentMajorVersionNumber");
                            if (majorObj != null)
                            {
                                // Windows 10 or later
                            }
                            else
                            {
                                // CurrentMajorVersionNumber doesn't exist on 8.1 and below
                                object verObj = key.GetValue("CurrentVersion");
                                if (verObj != null && verObj.ToString().StartsWith("6."))
                                {
                                    isUnsupported = true;
                                }
                            }
                        }
                    }
                }
                catch
                {
                    var os = Environment.OSVersion;
                    if (os.Platform == PlatformID.Win32NT && os.Version.Major == 6 && os.Version.Minor <= 3)
                        isUnsupported = true;
                }
            }

            warningIcon.Visible = isUnsupported;
            warningLabel.Visible = isUnsupported;
            
            // Adjust action button position based on warning visibility
            if (isUnsupported)
                actionButton.Location = new Point(10, 220);
            else
                actionButton.Location = new Point(10, 190);

            bool isInstalled = (name == "Live Server") ? ExtensionManager.IsLiveServerInstalled : ExtensionManager.IsDiscordRpcInstalled;
            UpdateActionButton(isInstalled, isUnsupported);

            listPanel.Visible = false;
            detailsPanel.Visible = true;
        }

        private void UpdateActionButton(bool isInstalled, bool isUnsupported)
        {
            if (isUnsupported)
            {
                actionButton.Enabled = false;
                actionButton.Text = "Not Supported";
                actionButton.BackColor = Color.FromArgb(60, 60, 60);
                actionButton.FlatAppearance.BorderColor = Color.FromArgb(60, 60, 60);
                return;
            }

            actionButton.Enabled = true;
            if (isInstalled)
            {
                actionButton.Text = "Uninstall";
                actionButton.BackColor = Color.FromArgb(100, 40, 40);
                actionButton.FlatAppearance.BorderColor = Color.FromArgb(100, 40, 40);
            }
            else
            {
                actionButton.Text = "Install";
                actionButton.BackColor = Color.FromArgb(14, 99, 156);
                actionButton.FlatAppearance.BorderColor = Color.FromArgb(14, 99, 156);
            }
        }

        private void OnActionClick(object sender, EventArgs e)
        {
            if (currentExtension == "Live Server")
                ExtensionManager.IsLiveServerInstalled = !ExtensionManager.IsLiveServerInstalled;
            else if (currentExtension == "Discord RPC")
                ExtensionManager.IsDiscordRpcInstalled = !ExtensionManager.IsDiscordRpcInstalled;

            bool isInstalled = (currentExtension == "Live Server") ? ExtensionManager.IsLiveServerInstalled : ExtensionManager.IsDiscordRpcInstalled;
            
            bool isUnsupported = false;
            if (currentExtension == "Discord RPC")
            {
                try
                {
                    using (var key = Microsoft.Win32.Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion"))
                    {
                        if (key != null && key.GetValue("CurrentMajorVersionNumber") == null)
                        {
                            object verObj = key.GetValue("CurrentVersion");
                            if (verObj != null && verObj.ToString().StartsWith("6."))
                                isUnsupported = true;
                        }
                    }
                }
                catch
                {
                    var os = Environment.OSVersion;
                    if (os.Platform == PlatformID.Win32NT && os.Version.Major == 6 && os.Version.Minor <= 3)
                        isUnsupported = true;
                }
            }

            UpdateActionButton(isInstalled, isUnsupported);
        }
    }
}
