README / 首次启动必读（macOS Gatekeeper）
========================================

【简体中文】

由于当前安装包未做苹果签名和公证，首次打开 ShareOneList 时，macOS 可能提示
"应用已损坏"或"无法打开"。这不是安装包损坏，而是 Gatekeeper 安全机制拦截了
未签名的应用。按下面任意一种方法处理一次即可，之后可正常使用。

方法一（推荐）：双击运行本 dmg 内的修复脚本
1. 双击 "fix-macos-gatekeeper.command"。
2. 若提示"无法验证开发者"：右键点击该文件 → 选"打开" → 再点"打开"确认。
3. 脚本会自动移除隔离属性，并尝试重新打开应用。
   若提示权限不足，请改用方法二中的 sudo 命令。

方法二：终端手动执行命令
1. 打开"终端"（聚焦搜索输入"终端"或 Terminal）。
2. 若已把应用拖入"应用程序"文件夹，执行：

   xattr -cr /Applications/ShareOneList.app
   open /Applications/ShareOneList.app

3. 若应用仍在其他位置，请把路径替换为 ShareOneList.app 的实际位置。
4. 若提示权限不足，在命令前加 sudo（需要输入开机密码）：

   sudo xattr -cr /Applications/ShareOneList.app

方法三：右键打开（部分 macOS 版本有效）
在"应用程序"文件夹中右键点击 ShareOneList → 选择"打开" → 在弹窗中再点
"打开"确认一次。

如有问题欢迎在 GitHub 提 Issue：https://github.com/ituff/ShareOneList/issues

========================================

[English]

The current builds are unsigned and not notarized. On first launch, macOS may
report "ShareOneList is damaged" or "cannot be opened". This is Gatekeeper
blocking an unsigned app — the package itself is fine. Any ONE of the fixes
below is enough; you only need to do it once.

Option 1 (recommended): run the bundled fix script
1. Double-click "fix-macos-gatekeeper.command".
2. If macOS says the file "cannot be verified": right-click the file →
   "Open" → "Open" again to confirm.
3. The script removes the quarantine attribute and re-opens the app.
   If it reports insufficient permissions, use the sudo command in Option 2.

Option 2: run the command manually in Terminal
1. Open Terminal (Spotlight search: Terminal).
2. If you moved the app into /Applications, run:

   xattr -cr /Applications/ShareOneList.app
   open /Applications/ShareOneList.app

3. If the app is somewhere else, replace the path with the real location.
4. If permission is denied, prefix the command with sudo (asks for your
   login password):

   sudo xattr -cr /Applications/ShareOneList.app

Option 3: right-click Open (works on some macOS versions)
In the Applications folder, right-click ShareOneList → "Open" → click
"Open" again in the dialog.

Issues: https://github.com/ituff/ShareOneList/issues
