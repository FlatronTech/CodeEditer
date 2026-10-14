using ScintillaNET;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Text;
using System.Windows.Forms;

namespace CodeEditer.Services
{
    public sealed class TabService : IDisposable
    {
        // ── Tab data ──────────────────────────────────────────────────────
        public sealed class TabInfo
        {
            public string FilePath;
            public Scintilla Editor;
            public bool IsNew;
            // Cached icon bitmap (copied from file so the file handle is released)
            public Bitmap Icon;
        }

        // ── Icon cache: extension -> Bitmap ──────────────────────────────
        private static readonly Dictionary<string, Bitmap> _iconCache =
            new Dictionary<string, Bitmap>(StringComparer.OrdinalIgnoreCase);

        private static Bitmap LoadIcon(string filePath)
        {
            string iconName;
            if (string.IsNullOrEmpty(filePath))
                iconName = null;
            else
            {
                string ext = Path.GetExtension(filePath).ToLowerInvariant();
                switch (ext)
                {
                    case ".py":   iconName = "py.png";     break;
                    case ".cs":   iconName = "csharp.png"; break;
                    case ".html":
                    case ".htm":  iconName = "html.png";   break;
                    case ".go":   iconName = "go.png";     break;
                    case ".css":  iconName = "css.png";    break;
                    case ".c":    iconName = "c.png";      break;
                    case ".cpp":
                    case ".cc":
                    case ".cxx":
                    case ".h":
                    case ".hpp":  iconName = "cplus.png";  break;
                    case ".js":   iconName = "js.png";     break;
                    case ".bat":
                    case ".cmd":  iconName = "bat.png";    break;
                    case ".php":  iconName = "php.png";    break;
                    case ".rs":   iconName = "rs.png";     break;
                    case ".java": iconName = "java.png";   break;
                    case ".rb":   iconName = "rb.png";     break;
                    case ".sql":  iconName = "sql.png";    break;
                    case ".xml":  iconName = "xml.png";    break;
                    case ".asm":
                    case ".s":    iconName = "asm.png";    break;
                    case ".ts":   iconName = "ts.png";     break;
                    case ".vbs":
                    case ".vb":   iconName = "vb.png";     break;
                    default:      iconName = null;         break;
                }
            }
            if (iconName == null) return null;

            Bitmap cached;
            if (_iconCache.TryGetValue(iconName, out cached))
                return cached;

            string iconPath = Path.Combine(Application.StartupPath, "icons", iconName);
            if (!File.Exists(iconPath)) return null;

            try
            {
                // Load into memory so file handle is immediately released
                using (var tmp = Image.FromFile(iconPath))
                {
                    var bmp = new Bitmap(16, 16);
                    using (var g = Graphics.FromImage(bmp))
                    {
                        g.InterpolationMode = InterpolationMode.HighQualityBicubic;
                        g.DrawImage(tmp, 0, 0, 16, 16);
                    }
                    _iconCache[iconName] = bmp;
                    return bmp;
                }
            }
            catch { return null; }
        }

        // ── Layout constants ──────────────────────────────────────────────
        private const int TAB_H        = 36;   // tab strip height
        private const int TAB_MIN_W    = 110;
        private const int TAB_MAX_W    = 240;
        private const int ICON_SIZE    = 16;
        private const int PAD_L        = 10;   // left padding before icon
        private const int ICON_TEXT_GAP = 5;
        private const int TEXT_CLOSE_GAP = 4;
        private const int CLOSE_W      = 18;   // close button width
        private const int PAD_R        = 6;    // right padding after close button

        // Colours
        private static readonly Color ColActive   = Color.FromArgb(30,  30,  30);
        private static readonly Color ColInactive = Color.FromArgb(51,  51,  51);
        private static readonly Color ColStrip    = Color.FromArgb(45,  45,  48);
        private static readonly Color ColTextAct  = Color.White;
        private static readonly Color ColTextInact= Color.FromArgb(160, 160, 160);
        private static readonly Color ColClose    = Color.FromArgb(150, 150, 150);
        private static readonly Color ColCloseHot = Color.FromArgb(200, 60,  60);

        // Fonts
        private static readonly Font _tabFont      = new Font("Segoe UI", 8.5f);
        private static readonly Font _closeFontSmall = new Font("Segoe UI", 7f);

        // ── State ─────────────────────────────────────────────────────────
        private readonly List<TabInfo> _tabs = new List<TabInfo>();
        private int _activeIndex = -1;
        private int _hotCloseIndex = -1;   // tab whose close button is hovered

        // Pre-computed tab rects (set in MeasureTabs)
        private Rectangle[] _tabRects = new Rectangle[0];
        private Rectangle[] _closeRects = new Rectangle[0];

        // ── Controls ──────────────────────────────────────────────────────
        private readonly Panel             _mainPanel;
        private readonly DoubleBufferedPanel _strip;
        private readonly Panel             _editorHost;

        // ── Events ────────────────────────────────────────────────────────
        public event Action<int>      ActiveTabChanged;
        public event Action<int>      TabClosed;
        public event Action<Scintilla> NewTabCreated;

        // ── Constructor ───────────────────────────────────────────────────
        public TabService()
        {
            _mainPanel = new Panel { Dock = DockStyle.Fill,
                                     BackColor = ColActive };

            _strip = new DoubleBufferedPanel { Dock = DockStyle.Top, Height = TAB_H,
                                               BackColor = ColStrip };
            _strip.Paint      += OnStripPaint;
            _strip.MouseMove  += OnStripMouseMove;
            _strip.MouseLeave += OnStripMouseLeave;
            _strip.MouseClick += OnStripMouseClick;

            _editorHost = new Panel { Dock = DockStyle.Fill,
                                      BackColor = ColActive };

            _mainPanel.Controls.Add(_editorHost);
            _mainPanel.Controls.Add(_strip);
        }

        // ── Public surface ────────────────────────────────────────────────
        public Panel  Control     { get { return _mainPanel; } }
        public int    TabCount    { get { return _tabs.Count; } }
        public int    ActiveIndex { get { return _activeIndex; } }

        public TabInfo GetActiveTab()
        {
            if (_activeIndex >= 0 && _activeIndex < _tabs.Count)
                return _tabs[_activeIndex];
            return null;
        }
        public TabInfo GetTabAt(int i)
        {
            return (i >= 0 && i < _tabs.Count) ? _tabs[i] : null;
        }
        public Scintilla GetCurrentEditor()
        {
            var t = GetActiveTab(); return t != null ? t.Editor : null;
        }

        public int FindTabByPath(string path)
        {
            for (int i = 0; i < _tabs.Count; i++)
                if (string.Equals(_tabs[i].FilePath, path,
                                  StringComparison.OrdinalIgnoreCase)) return i;
            return -1;
        }

        public int AddTab(string filePath)
        {
            if (!string.IsNullOrEmpty(filePath))
            {
                int ex = FindTabByPath(filePath);
                if (ex >= 0) { SwitchToTab(ex); return ex; }
            }

            var info = new TabInfo
            {
                FilePath = filePath,
                IsNew    = string.IsNullOrEmpty(filePath),
                Icon     = LoadIcon(filePath)
            };

            info.Editor = new Scintilla
            {
                Dock        = DockStyle.Fill,
                BorderStyle = BorderStyle.None,
                Visible     = false
            };

            if (!info.IsNew && File.Exists(filePath))
            {
                info.Editor.Text = File.ReadAllText(filePath, Encoding.UTF8);
                info.Editor.EmptyUndoBuffer();
                info.Editor.SetSavePoint();
            }

            _tabs.Add(info);
            _editorHost.Controls.Add(info.Editor);

            var handler = NewTabCreated;
            if (handler != null) handler(info.Editor);

            MeasureTabs();
            SwitchToTab(_tabs.Count - 1);
            return _tabs.Count - 1;
        }

        public void CloseTab(int index)
        {
            if (index < 0 || index >= _tabs.Count) return;

            var info = _tabs[index];
            _editorHost.Controls.Remove(info.Editor);
            info.Editor.Dispose();
            _tabs.RemoveAt(index);

            var closedHandler = TabClosed;
            if (closedHandler != null) closedHandler(index);

            if (_tabs.Count == 0)
            {
                _activeIndex = -1;
                MeasureTabs();
                _strip.Invalidate();
                var ch = ActiveTabChanged;
                if (ch != null) ch(-1);
            }
            else
            {
                if (_activeIndex >= _tabs.Count) _activeIndex = _tabs.Count - 1;
                else if (_activeIndex > index)   _activeIndex--;
                // if activeIndex == index, keep same index (now points to next tab)
                MeasureTabs();
                SwitchToTab(_activeIndex);
            }
        }

        public void SwitchToTab(int index)
        {
            if (index < 0 || index >= _tabs.Count) return;

            if (_activeIndex >= 0 && _activeIndex < _tabs.Count)
                _tabs[_activeIndex].Editor.Visible = false;

            _activeIndex = index;
            _tabs[_activeIndex].Editor.Visible = true;
            _tabs[_activeIndex].Editor.BringToFront();
            _tabs[_activeIndex].Editor.Focus();
            _strip.Invalidate();

            var handler = ActiveTabChanged;
            if (handler != null) handler(_activeIndex);
        }

        public void UpdateTabTitle(int index)
        {
            if (index < 0 || index >= _tabs.Count) return;
            MeasureTabs();
            _strip.Invalidate();
        }

        public bool SaveTab(int index)
        {
            if (index < 0 || index >= _tabs.Count) return false;
            var info = _tabs[index];
            if (info.IsNew || string.IsNullOrEmpty(info.FilePath)) return false;
            try
            {
                File.WriteAllText(info.FilePath, info.Editor.Text, new UTF8Encoding(false));
                info.Editor.SetSavePoint();
                UpdateTabTitle(index);
                return true;
            }
            catch { return false; }
        }

        public bool HasUnsavedChanges()
        {
            for (int i = 0; i < _tabs.Count; i++)
                if (_tabs[i].Editor.Modified) return true;
            return false;
        }

        public string GetTabDisplayName(int index)
        {
            if (index < 0 || index >= _tabs.Count) return "";
            var t = _tabs[index];
            string name = t.IsNew ? "Untitled" : Path.GetFileName(t.FilePath);
            return t.Editor.Modified ? "● " + name : name;
        }

        // ── Measure tab widths and positions ─────────────────────────────
        private void MeasureTabs()
        {
            _tabRects   = new Rectangle[_tabs.Count];
            _closeRects = new Rectangle[_tabs.Count];
            int x = 0;
            for (int i = 0; i < _tabs.Count; i++)
            {
                string label = GetTabDisplayName(i);
                int textW = TextRenderer.MeasureText(label, _tabFont).Width;
                int hasIcon = (_tabs[i].Icon != null) ? ICON_SIZE + ICON_TEXT_GAP : 0;
                int w = PAD_L + hasIcon + textW + TEXT_CLOSE_GAP + CLOSE_W + PAD_R;
                if (w < TAB_MIN_W) w = TAB_MIN_W;
                if (w > TAB_MAX_W) w = TAB_MAX_W;

                _tabRects[i]   = new Rectangle(x, 0, w, TAB_H);
                int closeX = x + w - PAD_R - CLOSE_W;
                _closeRects[i] = new Rectangle(closeX, (TAB_H - CLOSE_W) / 2,
                                               CLOSE_W, CLOSE_W);
                x += w + 1;
            }
        }

        // ── Owner-draw paint ─────────────────────────────────────────────
        private void OnStripPaint(object sender, PaintEventArgs e)
        {
            Graphics g = e.Graphics;
            g.SmoothingMode     = SmoothingMode.None;
            g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.ClearTypeGridFit;

            // Strip background
            g.Clear(ColStrip);

            for (int i = 0; i < _tabs.Count; i++)
            {
                bool active = (i == _activeIndex);
                Rectangle r = _tabRects[i];
                Color bgCol = active ? ColActive : ColInactive;

                // Tab background
                using (var br = new SolidBrush(bgCol))
                    g.FillRectangle(br, r);

                // Active tab top highlight line
                if (active)
                {
                    using (var pen = new Pen(Color.FromArgb(0, 122, 204), 2))
                        g.DrawLine(pen, r.Left, r.Top, r.Right - 1, r.Top);
                }

                // Icon
                int contentX = r.Left + PAD_L;
                Bitmap icon = _tabs[i].Icon;
                if (icon != null)
                {
                    int iconY = r.Top + (TAB_H - ICON_SIZE) / 2;
                    g.DrawImage(icon, contentX, iconY, ICON_SIZE, ICON_SIZE);
                    contentX += ICON_SIZE + ICON_TEXT_GAP;
                }

                // Title
                string label = GetTabDisplayName(i);
                Color textCol = active ? ColTextAct : ColTextInact;
                Rectangle closeR = _closeRects[i];
                int titleW = closeR.Left - contentX - TEXT_CLOSE_GAP;
                if (titleW > 0)
                {
                    Rectangle titleRect = new Rectangle(contentX,
                                                        r.Top,
                                                        titleW,
                                                        TAB_H);
                    TextRenderer.DrawText(g, label, _tabFont, titleRect,
                                         textCol,
                                         TextFormatFlags.VerticalCenter |
                                         TextFormatFlags.Left |
                                         TextFormatFlags.EndEllipsis |
                                         TextFormatFlags.NoPrefix);
                }

                // Close button
                bool closeHot = (i == _hotCloseIndex);
                Color closeCol = closeHot ? Color.White : ColClose;
                if (closeHot)
                {
                    using (var br = new SolidBrush(ColCloseHot))
                        g.FillRectangle(br, closeR);
                }
                TextRenderer.DrawText(g, "x", _closeFontSmall, closeR,
                                      closeCol,
                                      TextFormatFlags.HorizontalCenter |
                                      TextFormatFlags.VerticalCenter);

                // Separator between tabs
                if (i < _tabs.Count - 1)
                {
                    using (var pen = new Pen(ColStrip, 1))
                        g.DrawLine(pen, r.Right, r.Top + 4, r.Right, r.Bottom - 4);
                }
            }
        }

        // ── Mouse handling ────────────────────────────────────────────────
        private void OnStripMouseMove(object sender, MouseEventArgs e)
        {
            int newHot = -1;
            for (int i = 0; i < _closeRects.Length; i++)
                if (_closeRects[i].Contains(e.Location)) { newHot = i; break; }

            if (newHot != _hotCloseIndex)
            {
                _hotCloseIndex = newHot;
                _strip.Invalidate();
            }

            // Update cursor
            bool onClose = (newHot >= 0);
            _strip.Cursor = onClose ? Cursors.Hand : Cursors.Default;
        }

        private void OnStripMouseLeave(object sender, EventArgs e)
        {
            if (_hotCloseIndex != -1) { _hotCloseIndex = -1; _strip.Invalidate(); }
        }

        private void OnStripMouseClick(object sender, MouseEventArgs e)
        {
            for (int i = 0; i < _tabRects.Length; i++)
            {
                if (!_tabRects[i].Contains(e.Location)) continue;

                if (e.Button == MouseButtons.Middle)
                {
                    CloseTab(i);
                    return;
                }

                if (e.Button == MouseButtons.Left)
                {
                    if (_closeRects[i].Contains(e.Location))
                        CloseTab(i);
                    else
                        SwitchToTab(i);
                    return;
                }

                if (e.Button == MouseButtons.Right)
                {
                    ShowTabContextMenu(i, e.Location);
                    return;
                }
            }
        }

        private void ShowTabContextMenu(int tabIndex, Point location)
        {
            var ctx = new ContextMenuStrip();
            ctx.BackColor = Color.FromArgb(45, 45, 48);
            ctx.ForeColor = Color.White;
            ctx.Renderer  = new DarkContextRenderer();

            var closeItem = new ToolStripMenuItem("Close");
            closeItem.Click += delegate { CloseTab(tabIndex); };

            var closeOthers = new ToolStripMenuItem("Close Others");
            closeOthers.Click += delegate
            {
                for (int i = _tabs.Count - 1; i >= 0; i--)
                    if (i != tabIndex) CloseTab(i);
            };

            var closeAll = new ToolStripMenuItem("Close All");
            closeAll.Click += delegate
            {
                for (int i = _tabs.Count - 1; i >= 0; i--)
                    CloseTab(i);
            };

            ctx.Items.Add(closeItem);
            ctx.Items.Add(closeOthers);
            ctx.Items.Add(closeAll);
            ctx.Show(_strip, location);
        }

        // ── IDisposable ───────────────────────────────────────────────────
        public void Dispose()
        {
            foreach (var t in _tabs)
                t.Editor.Dispose();
            _tabs.Clear();
            // Don't dispose _iconCache bitmaps — they're shared across sessions
        }
    }

    // Dark renderer for right-click context menus on tabs
    internal class DarkContextRenderer : ToolStripProfessionalRenderer
    {
        public DarkContextRenderer() : base(new DarkContextColors()) { }
    }
    internal class DarkContextColors : ProfessionalColorTable
    {
        public override Color ToolStripDropDownBackground         { get { return Color.FromArgb(45, 45, 48); } }
        public override Color MenuItemSelected                    { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuItemBorder                      { get { return Color.FromArgb(62, 62, 64); } }
        public override Color MenuBorder                          { get { return Color.FromArgb(62, 62, 64); } }
        public override Color ImageMarginGradientBegin            { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ImageMarginGradientMiddle           { get { return Color.FromArgb(45, 45, 48); } }
        public override Color ImageMarginGradientEnd              { get { return Color.FromArgb(45, 45, 48); } }
        public override Color SeparatorDark                       { get { return Color.FromArgb(62, 62, 64); } }
        public override Color SeparatorLight                      { get { return Color.FromArgb(62, 62, 64); } }
    }

    // Double-buffered panel subclass (SetStyle is protected on Control)
    internal class DoubleBufferedPanel : Panel
    {
        public DoubleBufferedPanel()
        {
            SetStyle(ControlStyles.AllPaintingInWmPaint |
                     ControlStyles.UserPaint |
                     ControlStyles.OptimizedDoubleBuffer, true);
            UpdateStyles();
        }
    }
}
