English|[中文](README.md)
---
# VSRG-KeyVisualizer

<table>
  <tr>
    <td align="center"><img src="MD-PNG/image.png" width="150" /><br>Main Window</td>
    <td align="center"><img src="MD-PNG/image1.png" width="150" /><br>Configuration Window</td>
    <td align="center"><img src="MD-PNG/image2.png" width="150" /><br>Add Key</td>
  </tr>
</table>

VSRG-KeyVisualizer is a lightweight, real-time keyboard key visualization tool for VSRG (Vertical Scrolling Rhythm Games) players.

## Project Status

Currently supports Windows only.

Main window supports **drag to move**, **double-click to open config**, and **right-click context menu** (open config / close).

Still improving, but basically functional.

## Key Features

* **Real-time Performance**: Built with Rust and native Windows Raw Input, 60fps smooth rendering.
* **Key Visualization**: Waterfall flow animation, customizable key layout, color, size, and opacity.
* **Flexible Config**: Graphical configuration window with Drag & Drop key positioning.
* **Multi-Select Editing**: Ctrl+Click to select multiple keys, batch move/resize/color/opacity/bar width.
* **Snap & Collision**: Keys snap to alignment guides and avoid overlap during drag.
* **Transparent Window**: Main window has transparent background with click-through for uninterrupted gameplay.
* **Lightweight**: Minimal system resource usage, won't affect game performance.

## Tech Stack

* **Core Language**: Rust
* **UI Framework**: Slint 1.16
* **Input Capture**: Windows Raw Input API (low-latency keyboard hook)
* **Window Management**: winit window system

## Getting Started

### Prerequisites

Ensure [Rust programming environment](https://www.rust-lang.org/tools/install) (including Cargo) is installed.

### Build and Run

```bash
git clone https://github.com/lixiaapp/VSRG-KeyVisualizer.git
cd VSRG-KeyVisualizer
cargo run --release
```

## Usage

### Main Window
- **Left-click drag**: Move the window
- **Double-click**: Open configuration window
- **Right-click**: Context menu (open config / close)

### Configuration Window
- **Canvas drag**: Drag keys to reposition (snap-to-grid enabled)
- **Ctrl+Click**: Multi-select keys
- **Right panel**: Edit X/Y position, width/height, pressed color, opacity, bar width percentage
- **Multi-select editing**: Batch edit properties for all selected keys
- **Delete key / button**: Delete selected keys (with confirmation dialog, Enter to confirm, ESC to cancel)
- **+ Add Key**: Add a new key (press any key to bind)

### Configuration

Settings are saved in `config.json`, including key layout, colors, flow direction and speed. Can be adjusted graphically via the configuration window.

## License

This project is distributed under the **GNU General Public License v3.0 (GPLv3)**.

See the [LICENSE](LICENSE) file for details.
