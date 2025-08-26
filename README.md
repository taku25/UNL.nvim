# UNL.nvim

# Unreal Neovim Library 💓 Neovim

<table>
  <tr>
   <td><div align=center><img width="100%" alt="UCM New Class Interactive Demo" src="https://raw.githubusercontent.com/taku25/UNL.nvim/images/assets/top_image.png" /></div></td>
   </tr>
</table>

`UNL.nvim` (Unreal Neovim Library) is a shared, common library for a suite of plugins (`UEP.nvim`, `UCM.nvim`, `UBT.nvim`, etc.) designed to enhance Unreal Engine development on Neovim.

This library is designed to save plugin developers from writing boilerplate code when creating plugins for Unreal Engine, allowing them to focus on plugin-specific logic. Typically, it is not installed or configured directly by end-users but is managed automatically as a dependency of other plugins.

[English](https://www.google.com/search?q=./README.md) | [日本語 (Japanese)](https://www.google.com/search?q=./README_ja.md)

-----

## ✨ Features

  * **Layered Configuration Management**:
      * Provides a hierarchical configuration system that automatically merges plugin defaults, user's global settings, and project-local settings (`.unlrc.json`).
  * **UI Abstraction Backend**:
      * **Picker**: Automatically detects [Telescope](https://github.com/nvim-telescope/telescope.nvim) and [fzf-lua](https://github.com/ibhagwan/fzf-lua), with support for the native UI as a fallback.
      * **Progress**: Transparently handles multiple progress display methods, including [fidget.nvim](https://github.com/j-hui/fidget.nvim), custom windows, and notifications.
      * **Filer**: Automatically detects [neo-tree.nvim](https://github.com/nvim-neo-tree/neo-tree.nvim) and [nvim-tree](https://github.com/nvim-tree/nvim-tree.lua), with support for `netrw` as a fallback.
  * **Declarative Command Builder**:
      * A utility for easily creating Neovim user commands. It allows for declarative definitions of subcommands, argument parsing, `!` (bang) support, and completions.
  * **Advanced Finder Utilities**:
      * Provides an `ancestor finder` to search upwards through parent directories for directories containing `.uproject` files or module roots (`.build.cs`).
      * Includes logic to resolve the engine version from `.uproject` files.
  * **Analysis**:
      * Includes a built-in feature to parse `.build.cs` files and analyze their dependencies.
  * **Robust Logging System**:
      * Easily create per-plugin loggers with multiple output targets, such as files, notifications (`vim.notify`), and the command line (`echo`). Log levels and output formats are also flexible.

-----

## 🔧 Requirements

  * Neovim v0.11.3 or higher
  * (Optional) Various UI plugins to enhance the user experience (`Telescope`, `fidget.nvim`, etc.)

-----

## 🚀 Installation

Typically, users do not need to install this library manually, as it will be automatically installed as a dependency of other `Unreal Neovim` plugins.

Plugins using `lazy.nvim` should declare the dependency as follows:

```lua
-- Example: Installation configuration for UEP.nvim
return {
  'taku25/UEP.nvim',
  -- lazy.nvim will automatically install and manage UNL.nvim
  dependencies = { 'taku25/UNL.nvim' },
  opts = {
    -- Configuration is handled through the UNL.nvim system
  },
}
```

-----

## ⚙️ Configuration

`UNL.nvim` is the central hub for managing the configuration of itself and all plugins that use it. The `opts` table passed to the `setup` function in `lazy.nvim` will be merged with the default settings of all plugins and the `Localrc`.

The following are all available options with their default values:

```lua
opts = {
  -- Configuration for UI backends
  ui = {
    picker = {
      mode = "auto", -- "auto", "telescope", "fzf_lua", "native"
      prefer = { "telescope", "fzf_lua", "native" },
    },
    filer = {
      mode = "auto",
      prefer = { "nvim-tree", "neo-tree", "native" },
    },
    progress = {
      enable = true,
      mode = "auto", -- "auto", "fidget", "window", "notify"
      prefer = { "fidget", "window", "notify" },
    },
  },

  -- Configuration for logging
  logging = {
    level = "info", -- Global base log level (trace, debug, info, warn, error)
    echo = { level = "warn" }, -- Minimum level to display with :echo
    notify = { level = "error", prefix = "[UNL]" }, -- Minimum level and prefix for vim.notify
    file = { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" }, -- File log settings
  },

  -- Configuration for the cache directory
  cache = {
    -- The directory name where this library and related plugins
    -- will store cache files, i.e., <nvim_cache_dir>/<dirname>
    dirname = "UNL_cache"
  },

  -- Configuration for project searching
  project = {
    -- The filename for project-local settings
    localrc_filename = ".unlrc.json",
    -- If true, the search will not go above the home directory
    search_stop_at_home = true,
  },
}
```

-----

## 🤖 For Plugin Developers

Using `UNL.nvim` can significantly simplify your plugin development.

### Basic Usage

```lua
-- my_plugin/init.lua

-- Import core UNL modules
local unl_log = require("UNL.logging")
local unl_config = require("UNL.config")
-- Define your plugin's default settings
local my_defaults = require("my_plugin.config.defaults")

local M = {}

function M.setup(user_config)
  -- 1. Register your plugin with the UNL system
  -- This initializes the logger and configuration
  unl_log.setup("MyPlugin", my_defaults, user_config or {})

  -- 2. From here on, you can get the logger and config for your plugin
  local log = unl_log.get("MyPlugin")
  local conf = unl_config.get("MyPlugin")

  log.info("MyPlugin has been set up successfully!")
  log.debug("Current picker mode is: %s", conf.ui.picker.mode)
end

return M
```

-----

## 📜 License

MIT License

Copyright (c) 2025 taku25

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.