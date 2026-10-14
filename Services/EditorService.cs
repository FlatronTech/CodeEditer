using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Text;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using CodeEditer.Utils;
using ScintillaNET;

namespace CodeEditer.Services
{
    /// <summary>
    /// Configures a Scintilla editor instance: dark theme, line numbers,
    /// code folding, custom font, and per-language syntax highlighting.
    /// </summary>
    public sealed class EditorService
    {
        private static PrivateFontCollection fontCollection;
        private static string customFontName;

        static EditorService()
        {
            LoadCustomFont();
        }

        private readonly Scintilla editor;

        public EditorService(Scintilla editor)
        {
            this.editor = editor;
            Configure();
        }

        public Scintilla Control { get { return editor; } }

        /// <summary>
        /// Applies base dark-theme configuration: colors, margins, folding.
        /// Call once per editor instance.
        /// </summary>
        private void Configure()
        {
            string fontName = !string.IsNullOrEmpty(customFontName) ? customFontName : "Consolas";

            editor.StyleResetDefault();
            editor.Styles[Style.Default].BackColor = Color.FromArgb(30, 30, 30);
            editor.Styles[Style.Default].ForeColor = Color.FromArgb(212, 212, 212);
            editor.Styles[Style.Default].Font = fontName;
            editor.Styles[Style.Default].Size = 11;
            editor.StyleClearAll();

            editor.SetSelectionBackColor(true, Color.FromArgb(38, 79, 120));
            editor.CaretForeColor = Color.White;
            editor.CaretLineVisible = true;
            editor.CaretLineBackColor = Color.FromArgb(40, 40, 40);

            // Line numbers margin
            editor.Margins[0].Type = MarginType.Number;
            editor.Margins[0].Width = 44;
            editor.Margins[0].Mask = 0;
            editor.Styles[Style.LineNumber].BackColor = Color.FromArgb(30, 30, 30);
            editor.Styles[Style.LineNumber].ForeColor = Color.FromArgb(133, 133, 133);
            editor.Styles[Style.LineNumber].Font = fontName;
            editor.Styles[Style.LineNumber].Size = 9;

            // Folding margin
            editor.Margins[1].Width = 16;
            editor.Margins[1].Type = MarginType.Symbol;
            editor.Margins[1].Sensitive = true;
            editor.Margins[1].Mask = Marker.MaskFolders;

            editor.SetFoldMarginColor(true, Color.FromArgb(30, 30, 30));
            editor.SetFoldMarginHighlightColor(true, Color.FromArgb(30, 30, 30));

            // Fold markers
            editor.Markers[Marker.Folder].Symbol = MarkerSymbol.BoxPlus;
            editor.Markers[Marker.FolderOpen].Symbol = MarkerSymbol.BoxMinus;
            editor.Markers[Marker.FolderEnd].Symbol = MarkerSymbol.BoxPlusConnected;
            editor.Markers[Marker.FolderMidTail].Symbol = MarkerSymbol.TCorner;
            editor.Markers[Marker.FolderOpenMid].Symbol = MarkerSymbol.BoxMinusConnected;
            editor.Markers[Marker.FolderSub].Symbol = MarkerSymbol.VLine;
            editor.Markers[Marker.FolderTail].Symbol = MarkerSymbol.LCorner;

            for (int i = Marker.FolderEnd; i <= Marker.FolderOpen; i++)
            {
                editor.Markers[i].SetForeColor(Color.FromArgb(30, 30, 30));
                editor.Markers[i].SetBackColor(Color.FromArgb(150, 150, 150));
            }

            editor.AutomaticFold = AutomaticFold.Show | AutomaticFold.Click | AutomaticFold.Change;

            // No word wrap
            editor.WrapMode = WrapMode.None;

            // Tabs
            editor.TabWidth = 4;
            editor.UseTabs = false;
            editor.IndentationGuides = IndentView.LookBoth;

            // Brace matching
            editor.Styles[Style.BraceLight].BackColor = Color.FromArgb(60, 60, 60);
            editor.Styles[Style.BraceLight].ForeColor = Color.FromArgb(220, 220, 170);
            editor.Styles[Style.BraceBad].ForeColor = Color.FromArgb(244, 71, 71);

            // Problem detection & Auto Indentation & Intellisense
            editor.InsertCheck += Editor_InsertCheck;
            editor.TextChanged += Editor_TextChanged;
            editor.CharAdded += Editor_CharAdded;

            editor.Indicators[0].Style = IndicatorStyle.Squiggle;
            editor.Indicators[0].ForeColor = Color.Red;
        }

        private LanguageProfile currentProfile;

        private void Editor_InsertCheck(object sender, InsertCheckEventArgs e)
        {
            if (e.Text.EndsWith("\r") || e.Text.EndsWith("\n"))
            {
                int curLine = editor.LineFromPosition(editor.CurrentPosition);
                string curLineText = editor.Lines[curLine].Text;
                string indent = "";
                foreach (char c in curLineText)
                {
                    if (c == ' ' || c == '\t') indent += c;
                    else break;
                }
                e.Text += indent;
            }
        }

        private void Editor_CharAdded(object sender, CharAddedEventArgs e)
        {
            if (char.IsLetter((char)e.Char) || e.Char == '_')
            {
                var currentPos = editor.CurrentPosition;
                var wordStartPos = editor.WordStartPosition(currentPos, true);
                var lenEntered = currentPos - wordStartPos;

                if (lenEntered > 0)
                {
                    string currentWord = editor.GetTextRange(wordStartPos, lenEntered).ToLowerInvariant();
                    var list = new HashSet<string>();

                    if (currentProfile != null && !string.IsNullOrEmpty(currentProfile.Keywords))
                    {
                        foreach (var kw in currentProfile.Keywords.Split(' '))
                        {
                            if (kw.StartsWith(currentWord, StringComparison.OrdinalIgnoreCase))
                                list.Add(kw);
                        }
                    }

                    // Also search current document for words
                    string text = editor.Text;
                    var words = System.Text.RegularExpressions.Regex.Matches(text, @"\b[a-zA-Z_]\w*\b");
                    foreach (System.Text.RegularExpressions.Match m in words)
                    {
                        if (m.Value.StartsWith(currentWord, StringComparison.OrdinalIgnoreCase) && m.Value.Length > lenEntered)
                        {
                            list.Add(m.Value);
                        }
                    }

                    if (list.Count > 0)
                    {
                        var sortedList = new List<string>(list);
                        sortedList.Sort();
                        editor.AutoCMaxHeight = 10;
                        editor.AutoCShow(lenEntered, string.Join(" ", sortedList));
                    }
                }
            }
            else if (e.Char == '>')
            {
                if (currentProfile != null && (currentProfile.Name == "HTML" || currentProfile.Name == "XML"))
                {
                    var currentPos = editor.CurrentPosition;
                    int startPos = -1;
                    // Scan backwards up to 50 chars to find opening '<'
                    for (int i = currentPos - 2; i >= 0 && i >= currentPos - 50; i--)
                    {
                        char c = (char)editor.GetCharAt(i);
                        if (c == '>') break; // found another closing bracket before opening one
                        if (c == '<')
                        {
                            startPos = i;
                            break;
                        }
                    }

                    if (startPos != -1)
                    {
                        string tagContent = editor.GetTextRange(startPos + 1, currentPos - startPos - 2).Trim();
                        // Ignore self-closing tags, ending tags, or doctype/comments
                        if (tagContent.Length > 0 && !tagContent.StartsWith("/") && !tagContent.EndsWith("/") && !tagContent.StartsWith("!"))
                        {
                            string tagName = tagContent.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries)[0];
                            string t = tagName.ToLowerInvariant();
                            
                            // Skip common self-closing HTML elements
                            if (t != "br" && t != "hr" && t != "img" && t != "input" && t != "meta" && t != "link" && t != "source" && t != "area" && t != "col" && t != "base")
                            {
                                string closer = "</" + tagName + ">";
                                // Ensure we don't insert a duplicate closer
                                string textAfter = editor.GetTextRange(currentPos, closer.Length);
                                if (!string.Equals(textAfter, closer, StringComparison.OrdinalIgnoreCase))
                                {
                                    editor.InsertText(currentPos, closer);
                                }
                            }
                        }
                    }
                }
            }
        }

        private void Editor_TextChanged(object sender, EventArgs e)
        {
            editor.IndicatorClearRange(0, editor.TextLength);
            int totalBraces = 0, totalParens = 0, totalBrackets = 0;

            for (int i = 0; i < editor.Lines.Count; i++)
            {
                string text = editor.Lines[i].Text;
                int quoteCount = 0;
                int singleQuoteCount = 0;

                for (int j = 0; j < text.Length; j++)
                {
                    char c = text[j];
                    if (c == '"' && (j == 0 || text[j - 1] != '\\')) quoteCount++;
                    else if (c == '\'' && (j == 0 || text[j - 1] != '\\')) singleQuoteCount++;
                    else if (c == '{') totalBraces++;
                    else if (c == '}') totalBraces--;
                    else if (c == '(') totalParens++;
                    else if (c == ')') totalParens--;
                    else if (c == '[') totalBrackets++;
                    else if (c == ']') totalBrackets--;
                }

                // Check for unclosed string literals on a single line (rough heuristic)
                if ((quoteCount % 2 != 0) || (singleQuoteCount % 2 != 0))
                {
                    editor.IndicatorCurrent = 0;
                    editor.IndicatorFillRange(editor.Lines[i].Position, editor.Lines[i].Length);
                }
            }

            // If the document has unmatched brackets/braces overall, squiggle the end
            if (totalBraces != 0 || totalParens != 0 || totalBrackets != 0)
            {
                if (editor.Lines.Count > 0)
                {
                    var lastLine = editor.Lines[editor.Lines.Count - 1];
                    editor.IndicatorCurrent = 0;
                    editor.IndicatorFillRange(lastLine.Position, lastLine.Length);
                }
            }
        }

        /// <summary>
        /// Applies language-specific lexer, keywords, and style colors.
        /// </summary>
        public void ApplyProfile(LanguageProfile profile)
        {
            if (profile == null) return;
            currentProfile = profile;

            editor.Lexer = profile.Lexer;
            editor.SetKeywords(0, profile.Keywords);

            // Fold properties
            editor.SetProperty("fold", "1");
            editor.SetProperty("fold.compact", "1");
            editor.SetProperty("fold.html", "1");

            // Apply style colors from profile
            foreach (KeyValuePair<int, Color> style in profile.Styles)
            {
                editor.Styles[style.Key].ForeColor = style.Value;
            }
        }

        /// <summary>
        /// Configures a new Scintilla editor with the same dark theme settings.
        /// Used by TabService when creating new tabs.
        /// </summary>
        public static void ConfigureEditor(Scintilla editor)
        {
            EditorService svc = new EditorService(editor);
        }

        private static void LoadCustomFont()
        {
            try
            {
                string fontPath = Path.Combine(Application.StartupPath, "code.ttf");
                if (!File.Exists(fontPath))
                    return;

                fontCollection = new PrivateFontCollection();
                fontCollection.AddFontFile(fontPath);
                if (fontCollection.Families.Length > 0)
                {
                    customFontName = fontCollection.Families[0].Name;
                }
            }
            catch { }
        }
    }
}