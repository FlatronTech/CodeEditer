using CodeEditer.Services;
using System;
using System.Collections.Generic;
using System.IO;
using System.Windows.Forms;

namespace CodeEditer.Services
{
    /// <summary>
    /// Fills a TreeView lazily: folder contents are read only when
    /// the folder is expanded for the first time.
    /// </summary>
    public sealed class ExplorerService
    {
        private const string DummyNodeName = "__dummy__";

        private readonly TreeView treeView;
        private readonly FileIconService icons;

        public ExplorerService(TreeView treeView, FileIconService icons)
        {
            this.treeView = treeView;
            this.icons = icons;
            treeView.ImageList = icons.ImageList;
            treeView.BeforeExpand += OnBeforeExpand;
            treeView.NodeMouseDoubleClick += OnNodeMouseDoubleClick;
        }

        /// <summary>Raised when the user double-clicks a file. Argument: full path.</summary>
        public event Action<string> FileActivated;

        public void SetRootPath(string directoryPath)
        {
            treeView.BeginUpdate();
            try
            {
                treeView.Nodes.Clear();
                if (string.IsNullOrEmpty(directoryPath) || !Directory.Exists(directoryPath))
                    return;

                TreeNode root = new TreeNode(directoryPath);
                root.Tag = directoryPath;
                root.ImageIndex = icons.GetFolderIconIndex();
                root.SelectedImageIndex = root.ImageIndex;
                treeView.Nodes.Add(root);

                PopulateDirectory(root);
                root.Expand();
            }
            finally
            {
                treeView.EndUpdate();
            }
        }

        private void OnBeforeExpand(object sender, TreeViewCancelEventArgs e)
        {
            // A folder that still has only the dummy child was never scanned.
            if (e.Node.Nodes.Count == 1 && e.Node.Nodes[0].Name == DummyNodeName)
            {
                e.Node.Nodes.Clear();
                PopulateDirectory(e.Node);
            }
        }

        private void OnNodeMouseDoubleClick(object sender, TreeNodeMouseClickEventArgs e)
        {
            string path = e.Node.Tag as string;
            if (!string.IsNullOrEmpty(path) && File.Exists(path))
            {
                Action<string> handler = FileActivated;
                if (handler != null)
                    handler(path);
            }
        }

        private void PopulateDirectory(TreeNode parentNode)
        {
            string path = parentNode.Tag as string;
            if (string.IsNullOrEmpty(path))
                return;

            List<string> directories = new List<string>();
            List<string> files = new List<string>();

            try
            {
                directories.AddRange(Directory.GetDirectories(path));
                files.AddRange(Directory.GetFiles(path));
            }
            catch (Exception)
            {
                return; // Access denied or similar: leave the folder empty.
            }

            directories.Sort(StringComparer.OrdinalIgnoreCase);
            files.Sort(StringComparer.OrdinalIgnoreCase);

            foreach (string dir in directories)
            {
                TreeNode node = new TreeNode(Path.GetFileName(dir));
                node.Tag = dir;
                node.ImageIndex = icons.GetFolderIconIndex();
                node.SelectedImageIndex = node.ImageIndex;

                // Lazy placeholder; replaced on first expand.
                TreeNode dummy = new TreeNode(string.Empty);
                dummy.Name = DummyNodeName;
                node.Nodes.Add(dummy);

                parentNode.Nodes.Add(node);
            }

            foreach (string file in files)
            {
                TreeNode node = new TreeNode(Path.GetFileName(file));
                node.Tag = file;
                node.ImageIndex = icons.GetFileIconIndex(file);
                node.SelectedImageIndex = node.ImageIndex;
                parentNode.Nodes.Add(node);
            }
        }
    }
}