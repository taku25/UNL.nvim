-- Centralized default configuration for UNL.
local M = {
  ui = {
    picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua", "snacks", "native", "dummy" },
    },
    grep_picker = {
      mode = "auto",
      prefer = { "telescope", "fzf-lua", "snacks" }
    },
    filer = {
      mode = "auto",
      prefer = { "nvim-tree", "neo-tree", "native", "dummy"  },
    },
    progress = {
      mode = "auto",
      enable = true,
      prefer = { "fidget", "generic_status", "window", "notify", "dummy" },
      allow_regression = false,
    },
    debug_log = {
      position = "right",
      size = 0.4, -- 画面右側に40%の幅
    }, 
  },

  safe_open = {

    dashboard_filetypes = {
      "dashboard",
      "alpha",
      "starter",
      "snacks_dashboard",
    },    -- A list of buffer types to avoid.
    -- See `:help buftype` for more options.
    prevent_in_buftypes = {
      "quickfix",
      "help",
      "terminal",
      "prompt",
    },
    -- A list of file types to avoid.
    prevent_in_filetypes = {
      "neo-tree",
      "NvimTree",
      "TelescopePrompt",
      "fugitive",
      "lazy",
    },
  },

  logging = {
    level = "info",
    echo = { level = "warn" },
    notify = { level = "error", prefix = "[UNL]" },
    file = { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" },
    perf = { enabled = false, patterns = { "^refresh" }, level = "trace" },
    debug = { enable = true, },
  },

  vcs = {
    auto_refresh = {
      enabled = true,
      -- FocusGained でVCSハッシュをチェックするか
      on_focus = true,
      -- 連続チェックを防ぐクールダウン（秒）
      cooldown = 300,
      -- 変更ファイル数がこの閾値を超えたら Full Refresh に切り替え
      full_refresh_threshold = 100,
      -- これらにマッチするファイルが含まれていたら Full Refresh（モジュール構造変更の可能性）
      structural_patterns = { "%.uproject$", "%.Build%.cs$", "%.uplugin$", "%.Target%.cs$" },
    },
  },

  cache = { dirname = "UNL_cache" },
  project = {
    localrc_filename = ".unlrc.json",
    search_stop_at_home = true,
    follow_symlink = true,
  },
  remote = {
    host = "127.0.0.1",
    port = 30110,
    auto_server_start = true,
  },
}
return M
