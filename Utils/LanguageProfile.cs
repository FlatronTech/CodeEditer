using System;
using System.Collections.Generic;
using System.Drawing;
using ScintillaNET;

namespace CodeEditer.Utils
{
    /// <summary>
    /// Describes one language: which Scintilla lexer to use,
    /// keywords, and syntax-highlighting style colors.
    /// </summary>
    public sealed class LanguageProfile
    {
        private string[] keywordArray;

        public LanguageProfile(string name, Lexer lexer, string keywords, Dictionary<int, Color> styles)
        {
            Name = name;
            Lexer = lexer;
            Keywords = keywords ?? string.Empty;
            Styles = styles ?? new Dictionary<int, Color>();
        }

        public string Name { get; private set; }
        public Lexer Lexer { get; private set; }
        public string Keywords { get; private set; }
        public Dictionary<int, Color> Styles { get; private set; }

        /// <summary>Keywords as an array, built lazily once.</summary>
        public string[] KeywordArray
        {
            get
            {
                if (keywordArray == null)
                {
                    keywordArray = Keywords.Length == 0
                        ? new string[0]
                        : Keywords.Split(new char[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
                }
                return keywordArray;
            }
        }
    }
}