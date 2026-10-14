using System.Collections.Generic;
using System.Drawing;
using System.IO;
using ScintillaNET;

namespace CodeEditer.Utils
{
    public static class Languages
    {
        private static readonly LanguageProfile PlainText =
            new LanguageProfile("Plain Text", Lexer.Null, string.Empty, null);

        private static readonly LanguageProfile CSharp = new LanguageProfile(
            "C#", Lexer.Cpp,
            "abstract as base bool break byte case catch char checked class const continue " +
            "decimal default delegate do double else enum event explicit extern false finally " +
            "fixed float for foreach goto if implicit in int interface internal is lock long " +
            "namespace new null object operator out override params private protected public " +
            "readonly ref return sbyte sealed short sizeof stackalloc static string struct " +
            "switch this throw true try typeof uint ulong unchecked unsafe ushort using virtual " +
            "void volatile while var partial get set add remove value where yield " +
            "Console Math String Int32 Boolean List Dictionary Array Task IEnumerable Action Func DateTime TimeSpan",
            BuildCppDarkStyles());

        private static readonly LanguageProfile JavaScript = new LanguageProfile(
            "JavaScript", Lexer.Cpp,
            "abstract boolean break byte case catch char class const continue debugger default " +
            "delete do double else enum export extends false final finally float for function " +
            "goto if implements import in instanceof int interface let long native new null " +
            "package private protected public return short static super switch synchronized " +
            "this throw throws transient true try typeof var void volatile while with yield of " +
            "async await",
            BuildJsDarkStyles());

        private static readonly LanguageProfile Json =
            new LanguageProfile("JSON", Lexer.Cpp, "true false null", BuildCppDarkStyles());

        private static readonly LanguageProfile Html =
            new LanguageProfile("HTML", Lexer.Html, "a abbr address area article aside audio b base bdi bdo blockquote body br button canvas caption cite code col colgroup data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hr html i iframe img input ins kbd label legend li link main map mark meta meter nav noscript object ol optgroup option output p param picture pre progress q rp rt ruby s samp script section select small source span strong style sub summary sup svg table tbody td template textarea tfoot th thead time title tr track u ul var video wbr", BuildHtmlDarkStyles());

        private static readonly LanguageProfile Css =
            new LanguageProfile("CSS", Lexer.Css,
                "align-content align-items align-self animation animation-delay animation-direction " +
                "animation-duration animation-fill-mode animation-iteration-count animation-name " +
                "animation-play-state animation-timing-function background background-attachment " +
                "background-blend-mode background-clip background-color background-image " +
                "background-origin background-position background-repeat background-size " +
                "border border-bottom border-collapse border-color border-image border-left " +
                "border-radius border-right border-spacing border-style border-top border-width " +
                "bottom box-shadow box-sizing clear clip color column-count column-gap " +
                "column-rule column-width columns content counter-increment counter-reset " +
                "cursor direction display empty-cells filter flex flex-basis flex-direction " +
                "flex-flow flex-grow flex-shrink flex-wrap float font font-family font-size " +
                "font-style font-variant font-weight grid grid-area grid-auto-columns " +
                "grid-auto-flow grid-auto-rows grid-column grid-gap grid-row grid-template " +
                "grid-template-areas grid-template-columns grid-template-rows height hyphens " +
                "justify-content left letter-spacing line-height list-style list-style-image " +
                "list-style-position list-style-type margin margin-bottom margin-left margin-right " +
                "margin-top max-height max-width min-height min-width object-fit object-position " +
                "opacity order outline outline-color outline-offset outline-style outline-width " +
                "overflow overflow-x overflow-y padding padding-bottom padding-left padding-right " +
                "padding-top page-break-after page-break-before page-break-inside perspective " +
                "pointer-events position quotes resize right scroll-behavior tab-size table-layout " +
                "text-align text-decoration text-indent text-overflow text-shadow text-transform " +
                "top transform transition unicode-bidi user-select vertical-align visibility " +
                "white-space width word-break word-spacing word-wrap writing-mode z-index",
                BuildCssDarkStyles());

        private static readonly LanguageProfile C = new LanguageProfile("C", Lexer.Cpp, "auto break case char const continue default do double else enum extern float for goto if int long register return short signed sizeof static struct switch typedef union unsigned void volatile while", BuildCppDarkStyles());
        private static readonly LanguageProfile Cpp = new LanguageProfile("C++", Lexer.Cpp, "alignas alignof and and_eq asm atomic_cancel atomic_commit atomic_noexcept auto bitand bitor bool break case catch char char16_t char32_t class compl concept const constexpr const_cast continue decltype default delete do double dynamic_cast else enum explicit export extern false float for friend goto if inline int long mutable namespace new noexcept not not_eq nullptr operator or or_eq private protected public register reinterpret_cast requires return short signed sizeof static static_assert static_cast struct switch synchronized template this thread_local throw true try typedef typeid typename union unsigned using virtual void volatile wchar_t while xor xor_eq", BuildCppDarkStyles());
        private static readonly LanguageProfile Python = new LanguageProfile("Python", Lexer.Python, "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield print len str int float list dict set bool object type super open", BuildJsDarkStyles());
        private static readonly LanguageProfile Php = new LanguageProfile("PHP", Lexer.Cpp, "abstract and array as break callable case catch class clone const continue declare default die do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile eval exit extends final finally for foreach function global goto if implements include include_once instanceof insteadof interface isset list namespace new or print private protected public require require_once return static switch throw trait try unset use var while xor yield", BuildJsDarkStyles());
        private static readonly LanguageProfile Go = new LanguageProfile("Go", Lexer.Cpp, "break default func interface select case defer go map struct chan else goto package switch const fallthrough if range type continue for import return var", BuildCppDarkStyles());
        private static readonly LanguageProfile Batch = new LanguageProfile("Batch", Lexer.Batch, "rem set if else exist errorlevel for in do break call shift goto pushd popd echo", BuildJsDarkStyles());
        
        // New Languages
        private static readonly LanguageProfile Rust = new LanguageProfile("Rust", Lexer.Cpp, "as break const continue crate else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while async await dyn abstract become box do final macro override priv typeof unsized virtual yield", BuildCppDarkStyles());
        private static readonly LanguageProfile Java = new LanguageProfile("Java", Lexer.Cpp, "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized this throw throws transient try void volatile while true false null", BuildCppDarkStyles());
        private static readonly LanguageProfile Ruby = new LanguageProfile("Ruby", Lexer.Ruby, "alias and BEGIN begin break case class def defined? do else elsif END end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield", BuildJsDarkStyles());
        private static readonly LanguageProfile Sql = new LanguageProfile("SQL", Lexer.Sql, "select insert update delete from where and or not inner join left outer right full group by order having limit offset as into values set create table index view drop alter add primary key foreign references default unique check constraint null asc desc between like in", BuildCssDarkStyles());
        private static readonly LanguageProfile Xml = new LanguageProfile("XML", Lexer.Xml, string.Empty, BuildHtmlDarkStyles());
        private static readonly LanguageProfile Assembly = new LanguageProfile("Assembly", Lexer.Asm, "mov add sub cmp jmp je jne call ret push pop inc dec int loop shl shr and or xor test div mul lea", BuildCppDarkStyles());
        private static readonly LanguageProfile TypeScript = new LanguageProfile("TypeScript", Lexer.Cpp, "abstract any as boolean break case catch class console const constructor continue debugger declare default delete do double else enum export extends false finally for from function get if implements import in instanceof interface is let module namespace new null number of package private protected public require return set static string super switch symbol this throw true try type typeof undefined var void while with yield", BuildJsDarkStyles());
        private static readonly LanguageProfile VbScript = new LanguageProfile("VBScript", Lexer.Vb, "alias and as boolean byref byval call case catch class const continue date decimal default delegate dim do double each else elseif end enum erase error event exit false finally for friend function get global gosub goto handles if implements imports in inherits integer interface is let lib like long loop me mod module mustinherit mustoverride mybase myclass namespace narrowing new next not nothing notinheritable notoverridable object of on operator option optional or orelse overloads overridable overrides paramarray partial preserve private property protected public raiseevent readonly redim rem removehandler resume return select set shadows shared short single static step stop string structure sub synclock then throw to true try trycast typeof variant when while widening with withevents writeonly xor", BuildCppDarkStyles());

        public static LanguageProfile FromFileName(string fileName)
        {
            if (string.IsNullOrEmpty(fileName))
                return PlainText;

            string ext = Path.GetExtension(fileName).ToLowerInvariant();
            switch (ext)
            {
                case ".cs": return CSharp;
                case ".js": return JavaScript;
                case ".json": return Json;
                case ".htm":
                case ".html": return Html;
                case ".css": return Css;
                case ".c": return C;
                case ".cpp":
                case ".h":
                case ".hpp": return Cpp;
                case ".py": return Python;
                case ".php": return Php;
                case ".go": return Go;
                case ".bat":
                case ".cmd": return Batch;
                case ".rs": return Rust;
                case ".java": return Java;
                case ".rb": return Ruby;
                case ".sql": return Sql;
                case ".xml": return Xml;
                case ".asm":
                case ".s": return Assembly;
                case ".ts": return TypeScript;
                case ".vbs":
                case ".vb": return VbScript;
                default: return PlainText;
            }
        }

        // -- Dark theme style maps (VS Code inspired) --

        private static Dictionary<int, Color> BuildCppDarkStyles()
        {
            Dictionary<int, Color> s = new Dictionary<int, Color>();
            s[Style.Cpp.Default] = Color.FromArgb(212, 212, 212);
            s[Style.Cpp.Comment] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.CommentLine] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.CommentDoc] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.Number] = Color.FromArgb(181, 206, 168);
            s[Style.Cpp.Word] = Color.FromArgb(86, 156, 214);
            s[Style.Cpp.Word2] = Color.FromArgb(78, 201, 176);
            s[Style.Cpp.String] = Color.FromArgb(214, 157, 133);
            s[Style.Cpp.Character] = Color.FromArgb(214, 157, 133);
            s[Style.Cpp.Verbatim] = Color.FromArgb(214, 157, 133);
            s[Style.Cpp.Preprocessor] = Color.FromArgb(155, 155, 155);
            s[Style.Cpp.Operator] = Color.FromArgb(180, 180, 180);
            s[Style.Cpp.StringEol] = Color.FromArgb(214, 157, 133);
            return s;
        }

        private static Dictionary<int, Color> BuildJsDarkStyles()
        {
            Dictionary<int, Color> s = new Dictionary<int, Color>();
            s[Style.Cpp.Default] = Color.FromArgb(212, 212, 212);
            s[Style.Cpp.Comment] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.CommentLine] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.CommentDoc] = Color.FromArgb(106, 153, 85);
            s[Style.Cpp.Number] = Color.FromArgb(181, 206, 168);
            s[Style.Cpp.Word] = Color.FromArgb(197, 134, 192);
            s[Style.Cpp.Word2] = Color.FromArgb(220, 220, 170);
            s[Style.Cpp.String] = Color.FromArgb(206, 145, 120);
            s[Style.Cpp.Character] = Color.FromArgb(206, 145, 120);
            s[Style.Cpp.Operator] = Color.FromArgb(180, 180, 180);
            s[Style.Cpp.StringEol] = Color.FromArgb(206, 145, 120);
            return s;
        }

        private static Dictionary<int, Color> BuildHtmlDarkStyles()
        {
            Dictionary<int, Color> s = new Dictionary<int, Color>();
            s[Style.Html.Default] = Color.FromArgb(212, 212, 212);
            s[Style.Html.Tag] = Color.FromArgb(86, 156, 214);
            s[Style.Html.TagUnknown] = Color.FromArgb(86, 156, 214);
            s[Style.Html.Attribute] = Color.FromArgb(156, 220, 254);
            s[Style.Html.AttributeUnknown] = Color.FromArgb(156, 220, 254);
            s[Style.Html.Number] = Color.FromArgb(181, 206, 168);
            s[Style.Html.DoubleString] = Color.FromArgb(206, 145, 120);
            s[Style.Html.SingleString] = Color.FromArgb(206, 145, 120);
            s[Style.Html.Comment] = Color.FromArgb(106, 153, 85);
            s[Style.Html.Entity] = Color.FromArgb(78, 201, 176);
            s[Style.Html.TagEnd] = Color.FromArgb(86, 156, 214);
            return s;
        }

        private static Dictionary<int, Color> BuildCssDarkStyles()
        {
            Dictionary<int, Color> s = new Dictionary<int, Color>();
            s[Style.Css.Default] = Color.FromArgb(212, 212, 212);
            s[Style.Css.Tag] = Color.FromArgb(215, 186, 125);
            s[Style.Css.Class] = Color.FromArgb(215, 186, 125);
            s[Style.Css.PseudoClass] = Color.FromArgb(215, 186, 125);
            s[Style.Css.UnknownPseudoClass] = Color.FromArgb(215, 186, 125);
            s[Style.Css.Operator] = Color.FromArgb(180, 180, 180);
            s[Style.Css.Identifier] = Color.FromArgb(156, 220, 254);
            s[Style.Css.UnknownIdentifier] = Color.FromArgb(156, 220, 254);
            s[Style.Css.Value] = Color.FromArgb(206, 145, 120);
            s[Style.Css.Comment] = Color.FromArgb(106, 153, 85);
            s[Style.Css.Id] = Color.FromArgb(215, 186, 125);
            s[Style.Css.Important] = Color.FromArgb(197, 134, 192);
            s[Style.Css.Directive] = Color.FromArgb(197, 134, 192);
            s[Style.Css.DoubleString] = Color.FromArgb(206, 145, 120);
            s[Style.Css.SingleString] = Color.FromArgb(206, 145, 120);
            return s;
        }
    }
}