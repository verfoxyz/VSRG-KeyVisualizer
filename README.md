[English](README-EN.md)|中文
---
# VSRG-KeyVisualizer

![主窗口](MD-PNG/image.png)

![配置窗口](MD-PNG/image-1.png)

![新增按键](MD-PNG/image-2.png)

VSRG-KeyVisualizer 是一个轻量级、实时键盘按键显示工具，旨在为音游（VSRG）玩家提供直观的按键可视化反馈。

## 项目制作

目前只支持 Windows 系统。

主窗口双击打开配置界面，右键关闭应用！！！

还有bug，但基本可用，后续会添加更多功能。

## 项目简介

无论是在直播、录制视频还是进行日常练习，VSRG-KeyVisualizer 都能实时捕捉并显示您的键盘操作，帮助观众或您自己更清晰地观察击键过程。本项目使用 **Rust** 语言编写，并采用 **Slint** UI 框架开发，具有高性能和低资源占用的特点。

## 主要特性

* **实时性能**：基于 Rust 构建，确保按键触发与显示之间无感延迟。
* **轻量级**：系统资源占用极低，不影响您的游戏表现。
* **UI 框架**：使用 Slint UI，界面简洁且易于定制。
* **开源许可**：本项目遵循 GNU GPLv3 开源协议。

## 技术栈

* **核心语言**: Rust
* **UI 框架**: Slint

## 快速开始

### 前置要求

在编译或运行本项目之前，请确保您的系统中已安装：

* [Rust 编程环境](https://www.rust-lang.org/tools/install) (包括 Cargo)

### 编译运行

1. 克隆仓库到本地：
```bash
git clone https://github.com/lixiaapp/VSRG-KeyVisualizer.git
cd VSRG-KeyVisualizer

```


2. 运行项目：
```bash
cargo run

```



## 配置说明

您可以通过根目录下的 `config.json` 文件自定义按键显示的布局与样式（请根据项目实际配置项进行调整）。

## 开源协议

本项目采用 **GNU General Public License v3.0 (GPLv3)** 协议进行分发。这意味着：

* 您可以自由地使用、修改和分发本软件。
* 如果您对代码进行了修改并分发，您的衍生作品也必须在 GPLv3 协议下开源。

详情请参阅 [LICENSE](LICENSE) 文件。