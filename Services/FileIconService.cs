using CodeEditer.Utils;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace CodeEditer.Services
{
    /// <summary>
    /// Loads file/folder icons from the /icons directory (PNG files named by extension),
    /// with fallback to shell icons.
    /// </summary>
    public sealed class FileIconService : IDisposable
    {
        private readonly ImageList imageList = new ImageList();
        private readonly Dictionary<string, int> indexByKey =
            new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);

        private int folderIconIndex = -1;
        private int defaultFileIconIndex = -1;

        public FileIconService()
        {
            imageList.ColorDepth = ColorDepth.Depth32Bit;
            imageList.ImageSize = new Size(16, 16);
            LoadIconsFromDirectory();
        }

        public ImageList ImageList
        {
            get { return imageList; }
        }

        public int GetFolderIconIndex()
        {
            if (folderIconIndex == -1)
                folderIconIndex = AddShellIcon("Folder", true);
            return folderIconIndex;
        }

        public int GetFileIconIndex(string fileName)
        {
            string ext = Path.GetExtension(fileName ?? string.Empty).ToLowerInvariant();
            if (string.IsNullOrEmpty(ext))
                ext = ".file";

            int index;
            if (indexByKey.TryGetValue(ext, out index))
                return index;

            // Try shell icon
            index = AddShellIcon(ext, false);
            indexByKey[ext] = index;
            return index;
        }

        public void Dispose()
        {
            imageList.Dispose();
        }

        private void LoadIconsFromDirectory()
        {
            string iconDir = Path.Combine(Application.StartupPath, "icons");
            if (!Directory.Exists(iconDir))
                return;

            foreach (string file in Directory.GetFiles(iconDir, "*.png"))
            {
                string name = Path.GetFileNameWithoutExtension(file).ToLowerInvariant();
                string key;

                if (name == "folder" || name == "__folder__")
                    key = "__folder__";
                else if (name == "file" || name == "__default__")
                    key = "__default__";
                else
                    key = name.StartsWith(".") ? name : "." + name;

                try
                {
                    using (Image img = Image.FromFile(file))
                    {
                        Bitmap bmp = new Bitmap(img, 16, 16);
                        imageList.Images.Add(bmp);
                        int idx = imageList.Images.Count - 1;
                        indexByKey[key] = idx;

                        if (key == "__folder__")
                            folderIconIndex = idx;
                        else if (key == "__default__")
                            defaultFileIconIndex = idx;
                    }
                }
                catch { }
            }
        }

        private int AddShellIcon(string nameOrExtension, bool folder)
        {
            // Check if we have a custom icon loaded
            if (folder && folderIconIndex >= 0)
                return folderIconIndex;

            if (!folder)
            {
                int customIdx;
                if (indexByKey.TryGetValue(nameOrExtension, out customIdx))
                    return customIdx;
                if (defaultFileIconIndex >= 0)
                    return defaultFileIconIndex;
            }

            uint flags = NativeMethods.SHGFI_ICON |
                         NativeMethods.SHGFI_SMALLICON |
                         NativeMethods.SHGFI_USEFILEATTRIBUTES;

            uint attributes = folder
                ? NativeMethods.FILE_ATTRIBUTE_DIRECTORY
                : NativeMethods.FILE_ATTRIBUTE_NORMAL;

            NativeMethods.SHFILEINFO info;
            uint size = (uint)Marshal.SizeOf(typeof(NativeMethods.SHFILEINFO));
            IntPtr result = NativeMethods.SHGetFileInfo(
                nameOrExtension, attributes, out info, size, flags);

            if (result != IntPtr.Zero && info.hIcon != IntPtr.Zero)
            {
                try
                {
                    using (Icon original = Icon.FromHandle(info.hIcon))
                    using (Icon copy = (Icon)original.Clone())
                    {
                        imageList.Images.Add(copy);
                        return imageList.Images.Count - 1;
                    }
                }
                finally
                {
                    NativeMethods.DestroyIcon(info.hIcon);
                }
            }

            imageList.Images.Add(SystemIcons.Application);
            return imageList.Images.Count - 1;
        }
    }
}