using CodeEditer.Utils;
using ScintillaNET;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Text.RegularExpressions;
using System.Windows.Forms;

namespace CodeEditer.Services
{
    /// <summary>
    /// Custom autocomplete: language keywords + words found in the current
    /// document, shown in a focus-less popup ListBox near the caret.
    /// All keyboard handling stays on the editor side.
    /// </summary>
    public sealed class AutocompleteService : IDisposable
    {
        private const int MinPrefixLength = 2;
        private const int MaxSuggestions = 100;
        private const int MaxDocumentWords = 4000;
        private const int MaxWordLength = 64;
        private const int MaxDocumentScanLength = 2000000;
        private const int PopupWidth = 260;

        private static readonly Regex WordRegex =
            new Regex("[A-Za-z_][A-Za-z0-9_]*", RegexOptions.Compiled);

        private readonly Scintilla editor;
        private readonly PopupForm popup;
        private readonly ListBox listBox;

        private readonly HashSet<string> documentWords =
            new HashSet<string>(StringComparer.Ordinal);

        private string[] keywords = new string[0];
        private string currentPrefix = string.Empty;
        private int documentVersion;        // Incremented on every text change.
        private int cachedWordsVersion = -1; // Version the word cache was built from.

        public AutocompleteService(Scintilla editor)
        {
            this.editor = editor;

            listBox = new ListBox();
            listBox.BorderStyle = BorderStyle.None;
            listBox.Dock = DockStyle.Fill;
            listBox.Font = new Font("Consolas", 9F);
            listBox.SelectionMode = SelectionMode.One;
            listBox.DoubleClick += delegate { Commit(); };

            popup = new PopupForm();
            popup.Width = PopupWidth;
            popup.Controls.Add(listBox);

            editor.CharAdded += OnCharAdded;
            editor.TextChanged += OnTextChanged;
            editor.KeyDown += OnEditorKeyDown;
            editor.MouseDown += delegate { Hide(); };
            editor.LostFocus += delegate { Hide(); };
        }

        public void SetLanguage(LanguageProfile profile)
        {
            keywords = profile != null ? profile.KeywordArray : new string[0];
        }

        /// <summary>Opens the popup explicitly (Ctrl+Space / menu).</summary>
        public void Show(bool force)
        {
            currentPrefix = GetPrefix();
            if (!force && currentPrefix.Length < MinPrefixLength)
                return;

            RefreshSuggestions();

            if (listBox.Items.Count == 0 || (!force && HasSingleExactMatch()))
            {
                Hide();
                return;
            }

            PositionPopup();
            if (!popup.Visible)
                popup.Show();
        }

        public void Hide()
        {
            if (popup.Visible)
                popup.Hide();
        }

        /// <summary>Replaces the typed prefix with the selected word.</summary>
        public void Commit()
        {
            if (!popup.Visible || listBox.SelectedIndex < 0)
            {
                Hide();
                return;
            }

            string word = listBox.SelectedItem as string;
            if (word == null)
            {
                Hide();
                return;
            }

            int caret = editor.CurrentPosition;
            int wordStart = editor.WordStartPosition(caret, true);
            editor.SetSel(wordStart, caret);
            editor.ReplaceSelection(word);

            Hide();
            editor.Focus();
        }

        public void Dispose()
        {
            popup.Dispose();
        }

        // ------------------------------------------------------------------
        // Events
        // ------------------------------------------------------------------

        private void OnTextChanged(object sender, EventArgs e)
        {
            documentVersion++;
        }

        private void OnCharAdded(object sender, CharAddedEventArgs e)
        {
            if (popup.Visible)
            {
                RefreshAfterEdit();
            }
            else if (IsWordChar(e.Char)) // ⌨️ Tutaj wywoływana jest metoda z int
            {
                string prefix = GetPrefix();
                if (prefix.Length >= MinPrefixLength)
                    Show(false);
            }
        }

        // 🔄 POPRAWIONA METODA (zastąp tę z NotImplementedException)
        private bool IsWordChar(int @char)
        {
            char c = (char)@char; // 🔄 Rzutowanie int na char
            return char.IsLetterOrDigit(c) || c == '_'; // ✅ Sprawdzenie znaku
        }

        private void OnEditorKeyDown(object sender, KeyEventArgs e)
        {
            if (!popup.Visible)
            {
                if (e.Control && e.KeyCode == Keys.Space)
                {
                    Show(true);
                    e.Handled = true;
                    e.SuppressKeyPress = true;
                }
                return;
            }

            switch (e.KeyCode)
            {
                case Keys.Down:
                    MoveSelection(1);
                    break;
                case Keys.Up:
                    MoveSelection(-1);
                    break;
                case Keys.PageDown:
                    MoveSelection(VisibleRowCount());
                    break;
                case Keys.PageUp:
                    MoveSelection(-VisibleRowCount());
                    break;
                case Keys.Home:
                    SelectIndex(0);
                    break;
                case Keys.End:
                    SelectIndex(listBox.Items.Count - 1);
                    break;
                case Keys.Enter:
                case Keys.Tab:
                    Commit();
                    break;
                case Keys.Escape:
                    Hide();
                    break;
                case Keys.Back:
                case Keys.Delete:
                    // Let the edit happen, then re-filter on the next message loop turn.
                    editor.BeginInvoke(new Action(RefreshAfterEdit));
                    return;
                default:
                    return; // Normal typing: CharAdded re-filters.
            }

            e.Handled = true;
            e.SuppressKeyPress = true;
        }

        // ------------------------------------------------------------------
        // Filtering
        // ------------------------------------------------------------------

        private void RefreshAfterEdit()
        {
            if (!popup.Visible)
                return;

            currentPrefix = GetPrefix();
            if (currentPrefix.Length < MinPrefixLength)
            {
                Hide();
                return;
            }

            RefreshSuggestions();

            if (listBox.Items.Count == 0 || HasSingleExactMatch())
                Hide();
            else
                PositionPopup();
        }

        private void RefreshSuggestions()
        {
            RebuildDocumentWordsIfStale();

            List<string> keywordMatches = new List<string>();
            List<string> wordMatches = new List<string>();
            HashSet<string> seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

            // Keywords first.
            foreach (string keyword in keywords)
            {
                if (MatchesPrefix(keyword))
                {
                    keywordMatches.Add(keyword);
                    seen.Add(keyword);
                }
            }

            // Then words from the document.
            foreach (string word in documentWords)
            {
                if (wordMatches.Count >= MaxSuggestions)
                    break;
                if (!MatchesPrefix(word))
                    continue;
                if (seen.Contains(word))
                    continue;
                seen.Add(word);
                wordMatches.Add(word);
            }

            keywordMatches.Sort(StringComparer.OrdinalIgnoreCase);
            wordMatches.Sort(StringComparer.OrdinalIgnoreCase);

            listBox.BeginUpdate();
            listBox.Items.Clear();
            foreach (string s in keywordMatches) listBox.Items.Add(s);
            foreach (string s in wordMatches) listBox.Items.Add(s);
            listBox.EndUpdate();

            listBox.SelectedIndex = listBox.Items.Count > 0 ? 0 : -1;
        }

        private void RebuildDocumentWordsIfStale()
        {
            if (cachedWordsVersion == documentVersion)
                return;

            cachedWordsVersion = documentVersion;
            documentWords.Clear();

            string text = editor.Text;
            if (text.Length > MaxDocumentScanLength)
                return; // Safety cap for very large files.

            foreach (Match match in WordRegex.Matches(text))
            {
                if (match.Length > MaxWordLength)
                    continue;
                documentWords.Add(match.Value);
                if (documentWords.Count >= MaxDocumentWords)
                    break;
            }
        }

        private bool MatchesPrefix(string word)
        {
            if (currentPrefix.Length == 0)
                return true;
            return word.StartsWith(currentPrefix, StringComparison.OrdinalIgnoreCase);
        }

        private bool HasSingleExactMatch()
        {
            if (listBox.Items.Count != 1)
                return false;
            string only = listBox.Items[0] as string;
            return only != null &&
                string.Equals(only, currentPrefix, StringComparison.OrdinalIgnoreCase);
        }

        // ------------------------------------------------------------------
        // Helpers
        // ------------------------------------------------------------------

        private string GetPrefix()
        {
            int position = editor.CurrentPosition;
            int wordStart = editor.WordStartPosition(position, true);
            return editor.GetTextRange(wordStart, position - wordStart);
        }

        private static bool IsWordChar(char c)
        {
            return char.IsLetterOrDigit(c) || c == '_';
        }

        private int VisibleRowCount()
        {
            int itemHeight = listBox.ItemHeight;
            if (itemHeight <= 0)
                return 1;
            return Math.Max(1, listBox.ClientSize.Height / itemHeight);
        }

        private void MoveSelection(int delta)
        {
            if (listBox.Items.Count == 0)
                return;
            SelectIndex(listBox.SelectedIndex + delta);
        }

        private void SelectIndex(int index)
        {
            int count = listBox.Items.Count;
            if (count == 0)
                return;

            if (index < 0) index = 0;
            if (index >= count) index = count - 1;

            listBox.SelectedIndex = index;

            // Keep the selection scrolled into view.
            int top = listBox.TopIndex;
            int visibleRows = VisibleRowCount();
            if (index < top)
                listBox.TopIndex = index;
            else if (index >= top + visibleRows)
                listBox.TopIndex = index - visibleRows + 1;
        }

        private void PositionPopup()
        {
            int caret = editor.CurrentPosition;
            int x = editor.PointXFromPosition(caret);
            int y = editor.PointYFromPosition(caret);
            int line = editor.LineFromPosition(caret);

            // 🔄 Zastąpiono editor.TextHeight(line) bezpieczną alternatywą
            int lineHeight = listBox.ItemHeight > 0 ? listBox.ItemHeight : 16;

            Point screenPoint = editor.PointToScreen(new Point(x, y));
            int shownRows = Math.Min(listBox.Items.Count, 10);
            int popupHeight = shownRows * listBox.ItemHeight + 4;
            Screen screen = Screen.FromPoint(screenPoint);

            int px = screenPoint.X;
            int py = screenPoint.Y + lineHeight;

            if (py + popupHeight > screen.WorkingArea.Bottom)
                py = screenPoint.Y - popupHeight - 2;
            if (px + PopupWidth > screen.WorkingArea.Right)
                px = screen.WorkingArea.Right - PopupWidth;

            popup.Location = new Point(px, py);
            popup.Size = new Size(PopupWidth, popupHeight);
        }

        // ------------------------------------------------------------------
        // Popup form that never activates and never steals focus.
        // ------------------------------------------------------------------

        private sealed class PopupForm : Form
        {
            public PopupForm()
            {
                FormBorderStyle = FormBorderStyle.None;
                StartPosition = FormStartPosition.Manual;
                ShowInTaskbar = false;
                TopMost = true;
                BackColor = SystemColors.Window;
                Padding = new Padding(2);
            }

            protected override bool ShowWithoutActivation
            {
                get { return true; }
            }

            protected override CreateParams CreateParams
            {
                get
                {
                    CreateParams cp = base.CreateParams;
                    cp.ExStyle |= 0x08000000; // WS_EX_NOACTIVATE
                    cp.ExStyle |= 0x00000080; // WS_EX_TOOLWINDOW
                    return cp;
                }
            }
        }
    }
}