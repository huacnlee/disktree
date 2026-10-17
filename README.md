# Disktree

用 **nsh** 构建的原生磁盘空间分析工具，灵感来自 [tobi/disktree](https://github.com/tobi/disktree)。支持 macOS 和 Linux。

- 自动扫描上次浏览的目录，首次打开扫描主目录。
- 彩色嵌套树图：代码、Git、缓存、构建、工具链、媒体等分类。
- MacBook 双指捏合缩放，拖动平移；`⌘+` / `⌘-`（Linux 使用 Ctrl）缩放，`⌘0` 适配。
- 单击选中，再次点击同一文件夹直接进入；面包屑单击切换。
- 鼠标指向区块后按 Space 标记或取消；键盘也可用方向键选择。
- 标记项显示琥珀色、斜纹和勾选；审核后移入系统废纸篓。
- 完整扫描索引保留在本地服务，界面按页读取，避免百万文件撑满 JS 堆。

## 安装

发布包包含原生程序和扫描服务，无需安装 Python、Node 或 nsh。

### mise

```sh
mise use -g github:huacnlee/nsh-disktree
# 或仅安装：mise install github:huacnlee/nsh-disktree@latest
disktree
```

使用 mise 的 [GitHub backend](https://mise.jdx.dev/dev-tools/backends/github.html)，自动选择当前平台的发布包。

### curl

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/nsh-disktree/main/install.sh | sh
```

安装指定版本：

```sh
curl -fsSL https://raw.githubusercontent.com/huacnlee/nsh-disktree/main/install.sh | sh -s -- v0.1.0
```

安装器验证 Release 的 SHA-256 校验和，默认将命令放在 `~/.local/bin/disktree`，macOS 同时在 `~/Applications` 创建应用入口。可用 `DISKTREE_BIN_DIR`、`DISKTREE_INSTALL_DIR` 和 `DISKTREE_APPS_DIR` 更改位置。升级时重复运行安装命令。

### 手动下载

从 [Releases](https://github.com/huacnlee/nsh-disktree/releases) 下载对应架构的 `.tar.gz`，完整解压后运行 `./disktree`；macOS 也可打开 `Disktree.app`。请保留应用及其配套资源的目录结构。

macOS 要求 14+。当前发布包采用 ad hoc 签名，尚未公证；首次打开可能需要在“系统设置 → 隐私与安全性”中允许。Linux 要求 GTK 4.14+，例如 Ubuntu 24.04：

```sh
sudo apt install libgtk-4-1 libglib2.0-bin
```

## 使用

| 操作 | 方式 |
| --- | --- |
| 扫描目录 | 顶部输入目录，按 Enter 或点击 Scan folder |
| 选中区块 | 单击或方向键 |
| 进入文件夹 | 已选中文件夹再次点击，或 Enter / Open folder |
| 返回 | 面包屑单击，或 Alt+↑ |
| 放大 / 缩小 | 双指张开 / 合拢，缩放按钮，或 Cmd/Ctrl + / - |
| 平移地图 | 按住鼠标拖动 |
| 重置缩放 | 点击百分比或 Cmd/Ctrl+0 |
| 标记 / 取消标记 | 鼠标指向区块后按 Space，或 Mark / unmark |
| 清理 | Review marked items → 审核完整路径 → Confirm move to Trash |

标记不会立即删除文件。应用不会清空废纸篓。文件移入废纸篓后需重新扫描更新地图，清空废纸篓后才能实际释放空间。

目录列表每页 80 项。搜索覆盖当前目录的所有直接子项；其他页或未展开细节在地图中汇总显示，保留总量，不能直接标记汇总块。放大后可进入具体文件夹继续查看。

目录位置保存在 macOS 的 `~/Library/Application Support/Disktree/state.json`，Linux 的 `${XDG_STATE_HOME:-~/.local/state}/nsh-disktree/state.json`。`DISKTREE_ROOT` 可覆盖启动目录，`DISKTREE_STATE` 可覆盖状态文件位置。

## 开发

安装 [nsh](https://github.com/longbridge/native-shell) 和 Python 3，然后：

```sh
./scripts/run.sh
```

桌面悬停事件使用仓库中的 `patches/nsh-hover.patch`。发布构建会对固定版本的 nsh 应用补丁；开发原生宿主也需使用该版本。构建自包含应用需要 Rust、Python 3.11+，macOS 需要 Xcode Command Line Tools，Linux 需要 `clang libclang-dev libgtk-4-dev`。

```sh
python3 -m venv .venv
.venv/bin/pip install pyinstaller==6.16.0
.venv/bin/python scripts/build-release.py --version 0.1.0
```

首次构建会下载固定版本的 nsh 源码，输出到 `dist/`。原生启动器在每次运行时启动附带的扫描器，读取随机端口和随机令牌，然后将连接注入 nsh JS bundle；安装目录保持只读，退出窗口时回收扫描进程。

检查：

```sh
python3 -m unittest discover -s scripts -p 'test_*.py' -v
python3 scripts/stress_memory.py 650000 6
nsh check
```

`connection.ts` 是运行时生成的开发配置，不提交到仓库；单独做类型检查前可从 `connection.example.ts` 复制生成。

## 发布

`.github/workflows/release.yml` 在版本标签推送后构建 macOS 和 Linux 发布包，运行扫描回归测试及原生窗口 smoke test，再上传压缩包和 `SHA256SUMS` 到 GitHub Release。

```sh
git tag v0.1.0
git push origin v0.1.0
```

也可手动运行 workflow，仅构建和上传 CI artifacts，不发布 Release。

## 计量说明

Disk size 统计分配块，Apparent 统计逻辑字节，Files 按文件数分配面积。硬链接的字节只计一次；符号链接、其他设备和云端占位目录不会遍历。权限不足的路径会被报告，扫描结果可能不完整。APFS 克隆文件按各自大小计量，系统可清理空间和快照不会算作扫描文件。macOS 受保护目录需要为应用或开发终端授予完整磁盘访问权限。

MIT License.
