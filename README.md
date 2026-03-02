# UNL.nvim

# Unreal Neovim Library 💓 Neovim

<table>
  <tr>
   <td><div align=center><img width="100%" alt="UCM New Class Interactive Demo" src="https://raw.githubusercontent.com/taku25/UNL.nvim/images/assets/top_image.png" /></div></td>
   </tr>
</table>

`UNL.nvim` (Unreal Neovim Library) is a shared, common library for a suite of plugins ([`UEP.nvim`](https://github.com/taku25/UEP.nvim), [`UCM.nvim`](https://github.com/taku25/UCM.nvim), [`UBT.nvim`](https://github.com/taku25/UBT.nvim), etc.) designed to enhance Unreal Engine development on Neovim.

This library is designed to save plugin developers from writing boilerplate code when creating plugins for Unreal Engine, allowing them to focus on plugin-specific logic. Typically, it is not installed or configured directly by end-users but is managed automatically as a dependency of other plugins.

[English](README.md) | [日本語 (Japanese)](README_ja.md)

-----

## ✨ Features

  * **Layered Configuration Management**:
      * Provides a hierarchical configuration system that automatically merges plugin defaults, user's global settings, and project-local settings (`.unlrc.json`).
  * **UI Abstraction Backend**:
      * **Picker**: Automatically detects [Telescope](https://github.com/nvim-telescope/telescope.nvim) ,[fzf-lua](https://github.com/ibhagwan/fzf-lua) and [snacks](https://github.com/folke/snacks.nvim), with support for the native UI as a fallback.
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
  * **Project Management & RPC Server**:
      * Centralizes the management of the Rust-based RPC server (`unl-server`) via the `:UNL start` command. It prevents multiple instances and allows safe sharing across multiple Neovim sessions.
      * Handles project-specific database initialization and registration via `:UNL setup`.
      * Provides server-side file filtering and symbol search APIs for high performance even in massive projects.
  * **High-Performance Scanner (Rust)**:
      * Includes a built-in Rust-based binary scanner for lightning-fast C++ header analysis. It utilizes Tree-sitter for accurate parsing of Unreal Engine macros and class structures.

-----

## 🔧 Requirements

  * Neovim v0.11.3 or higher
  * [Rust and Cargo](https://www.rust-lang.org/tools/install) (Required to build the scanner binary)
  * (Optional) Various UI plugins to enhance the user experience (`Telescope`, `fzf-lua`, etc.)

-----

## 🚀 Installation

Typically, users do not need to install this library manually, as it will be automatically installed as a dependency of other `Unreal Neovim` plugins.

However, since this plugin includes a Rust-based scanner, you need to add a **build hook** to compile the binary during installation or updates.

### Using [lazy.nvim](https://github.com/folke/lazy.nvim)

```lua
return {
  'taku25/UNL.nvim',
  build = "cargo build --release --manifest-path scanner/Cargo.toml",
}
```

### Manual Build

If the automatic build fails, you can manually build the scanner by running the following command in the plugin's root directory:

```bash
cargo build --release --manifest-path scanner/Cargo.toml
```

Example installation configuration for `UEP.nvim`:

```lua
return {
  'taku25/UEP.nvim',
  -- lazy.nvim will automatically install and manage UNL.nvim
  dependencies = {
    { 'taku25/UNL.nvim', build = "cargo build --release --manifest-path scanner/Cargo.toml" }
  },
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
      behaviour = {
        single = "native",
        multiselect = "native",
        multiselect_empty = "confirm_item",
      },
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

## 🤖 API & Automation

You can control UNL programmatically via `require("UNL.api")`. This is useful for creating custom keymaps or integrating with other plugins.

```lua
local unl = require("UNL.api")

-- Start the UNL server and file watcher
unl.start()

-- Refresh the project database (scans files and updates DB)
-- scope: "Game", "Engine", "Full"
unl.refresh({ scope = "Game" })

-- Setup UNL for the current project (usually called by start, but can be manual)
unl.setup()

-- Start the file watcher explicitly
unl.watch()

-- Search files using server-side filtering (High Performance)
-- modules: list of module names to search in
-- filter: keyword to match file paths
-- limit: max number of results
unl.db.search_files_in_modules({"Core", "Engine"}, "Actor", 100, function(files)
  for _, file in ipairs(files) do
    print(file.file_path)
  end
end)
```

-----

## 🎨 Unified Picker API

Use `require("UNL.picker").open(spec)` to display the best picker based on the user's environment (Telescope, fzf-lua, Snacks, etc.). This allows plugin developers to provide a consistent rich UI without worrying about backend differences.

### Basic Usage

```lua
local unl_picker = require("UNL.picker")

unl_picker.open({
  title = "Select Module",
  items = { "Core", "Engine", "Project" }, -- Static list
  on_confirm = function(selection)
    print("Selected: " .. selection)
  end,
})
```

### Spec (Options) Definition

The `spec` table passed to `open()` supports the following fields:

| Field | Type | Description |
| :--- | :--- | :--- |
| `title` | `string` | The title of the picker. |
| `source` | `table` | Data source definition (see below). |
| `items` | `table` | Shorthand for `source.type = "static"`. A list of items. |
| `on_confirm` | `function` | Callback when an item is confirmed. Receives the selection. |
| `multiselect` | `string/bool` | Selection mode: `false` or `"single"` (single selection), `true` or `"multiselect"` (multi selection enabled, no empty selection), or `"multiselect_empty"` (multi selection, empty selection allowed). |
| `preview_enabled`| `boolean` | Whether to enable the previewer. |
| `devicons_enabled`| `boolean` | Whether to show icons. |
| `default_selected`| `boolean` | If multiselection, should the entries be selected by default |

### Source Type Variations

You can handle different data formats by specifying the `source` field:

*   **`static`**: Displays a fixed list.
    ```lua
    source = { type = "static", items = { { label = "Item 1", value = 1 }, ... } }
    ```
*   **`grep`**: Performs a dynamic search like `live_grep`.
    ```lua
    source = { 
      type = "grep", 
      search_paths = { "Source/Runtime" }, 
      include_extensions = { "h", "cpp" } 
    }
    ```
*   **`job`**: Runs an external command (e.g., `fd`) and lists its output.
    ```lua
    source = { type = "job", command = { "fd", "--type", "f", "." } }
    ```
*   **`callback`**: Dynamically push items via a function.
    ```lua
    source = {
      type = "callback",
      fn = function(push)
        push({ "Item A", "Item B" }) -- Push a list
        -- Supports asynchronous updates
        some_async_request(function(data) push(data) end)
      end
    }
    ```

### Injecting Custom Pickers

Users can use a completely custom picker implementation by passing a function to `ui.picker.mode`.

```lua
require('UNL').setup({
  ui = {
    picker = {
      mode = function(spec)
        -- Interpret the spec and show your preferred UI (e.g., mini.pick)
        require('mini.pick').start({
          source = { items = spec.items, name = spec.title },
          callback = spec.on_confirm
        })
      end
    }
  }
})
```

### Modifying the behaviours

It is possible to customize the behaviours (`single`, `multiselect`, `multiselect_empty`) of the pickers in the configuration. Here is the valid options:

| Behaviour | Value | Description |
| :--- | :--- | :--- |
| `single` | `native` | Only valid option for `single` |
| `multiselect` | `native` | Will return the selected entries. If none are selected return the current entry |
| `multiselect` | `loop` | Selecting an entry will check it. Selecting `* confirm selection` will return the checked entries. |
| `multiselect_empty` | `native` | Will return the selected entries. If none are selected return an empty selection (fallback to loop for fzf-lua) |
| `multiselect_empty` | `confirm_item` | Added a `* Confirm selection` entry. On confirm, return an empty selection if `* Confirm selection` was not selected, or else return the other selected items |
| `multiselect_empty` | `loop` | Selecting an entry will check it. Selecting `* confirm selection` will return the checked entries. |

It is also possible to further customize the behaviour by providing a function instead of a string.

```lua
require("UNL").setup({
  ui = {
    picker = {
      behaviour = {
        single = function (opts, spec)
          ...
        end
      }
    }
  }
})
```

`spec` is a table containing the arguments in the call to `require("UNL.picker").open({...})`. `opts` is a table holding the arguments that will be provided to the picker. The function should modify `opts` to obtain the desired behaviours. Examples can be found inside `lua/UNL/backend/picker/provider`.

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
