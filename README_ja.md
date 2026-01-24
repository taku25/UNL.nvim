# UNL.nvim

# Unreal Neovim Library 💓 Neovim

<table>
  <tr>
   <td><div align=center><img width="100%" alt="UCM New Class Interactive Demo" src="https://raw.githubusercontent.com/taku25/UNL.nvim/images/assets/top_image.png" /></div></td>
   </tr>
</table>

`UNL.nvim` (Unreal Neovim Library) は、Neovim上でUnreal Engine開発を強化するためのプラグイン群（[`UEP.nvim`](https://github.com/taku25/UEP.nvim), [`UCM.nvim`](https://github.com/taku25/UCM.nvim), [`UBT.nvim`](https://github.com/taku25/UBT.nvim)等）のための、共通化された共有ライブラリです。

このライブラリは、プラグイン開発者がUnreal Engienのプラグインを作る上で無駄なコードを書く手間を省き、Plugin特有のロジック開発に集中できるように設計されています。通常、エンドユーザーが直接インストールや設定を行うことはなく、他のプラグインの依存関係として自動的に管理されます。

[English](README.md) | [日本語 (Japanese)](README_ja.md)

-----

## ✨ 機能 (Features)

  * **レイヤー化された設定管理**:
      * プラグインのデフォルト設定、ユーザーのグローバル設定、プロジェクトローカルの設定 (`.unlrc.json`) を自動的にマージする、階層化された設定システムを提供します。
  * **UI抽象化バックエンド**:
      * **Picker** (選択UI): [Telescope](https://github.com/nvim-telescope/telescope.nvim),[fzf-lua](https://github.com/ibhagwan/fzf-lua)や[snacks](https://github.com/folke/snacks.nvim)を自動検出し、フォールバックとしてネイティブUIもサポートします。
      * **Progress** (進捗UI): [fidget.nvim](https://github.com/j-hui/fidget.nvim)やカスタムウィンドウ、通知など、複数の進捗表示方法を透過的に扱えます。
      * **Filer** (ファイラーUI): [neo-tree.nvim](https://github.com/nvim-neo-tree/neo-tree.nvim)や[nvim-tree](https://github.com/nvim-tree/nvim-tree.lua)を自動検出し、フォールバックとしてnetrwUIもサポートします。
  * **宣言的なコマンドビルダー**:
      * Neovimのユーザーコマンドを簡単に作成するためのユーティリティです。サブコマンド、引数パーシング、`!`(bang)対応、補完などを宣言的に定義できます。
  * **高度なファインダーユーティリティ**:
      * `.uproject`ファイルやモジュールルート (`.build.cs`) を持つディレクトリを、親ディレクトリを遡って探索する`ancestor finder`を提供します。
      * `.uproject`ファイルからエンジンバージョンを解決するロジックも内蔵しています。
  * **解析**
      * `.build.cs`ファイルの解析を行い依存関係を解析機能を内蔵しています
  * **堅牢なロギングシステム**:
      * ファイル、通知 (`vim.notify`)、コマンドライン (`echo`) など、複数の出力先を持つロガーをプラグインごとに簡単に作成できます。ログレベルや出力形式も柔軟に設定可能です。
  * **高速スキャナ (Rust製)**:
      * C++ヘッダーを解析するための、Rust製の超高速バイナリスキャナを内蔵しています。Tree-sitterを利用して、Unreal Engineのマクロやクラス構造を正確に解析します。

## 🔧 必要要件 (Requirements)

  * Neovim v0.11.3 以上
  * [Rust と Cargo](https://www.rust-lang.org/tools/install) (スキャナのバイナリをビルドするために必要)
  * (任意) UI体験を向上させるための各種UIプラグイン (`Telescope`, `fzf-lua`など)

## 🚀 インストール (Installation)

通常、このライブラリは他の`Unreal Neovim`プラグインの依存関係として自動的にインストールされるため、ユーザーが手動でインストールする必要はありません。

ただし、Rust製のスキャナが含まれているため、インストール時や更新時にバイナリをコンパイルするための **ビルドフック** を追加する必要があります。

### [lazy.nvim](https://github.com/folke/lazy.nvim) を使用する場合

```lua
return {
  'taku25/UNL.nvim',
  build = "cargo build --release --manifest-path scanner/Cargo.toml",
}
```

`UEP.nvim` などの依存プラグインを通じてインストールする場合の例：

```lua
return {
  'taku25/UEP.nvim',
  -- dependencies 内で UNL.nvim に対してビルドフックを指定します
  dependencies = { 
    { 'taku25/UNL.nvim', build = "cargo build --release --manifest-path scanner/Cargo.toml" } 
  },
  opts = {
    -- 設定はUNL.nvimのシステムを通じて行われます
  },
}
```

## ⚙️ 設定 (Configuration)

`UNL.nvim`は、自身とそれを利用する全てのプラグインの設定を管理する中央ハブです。`lazy.nvim`の`setup`関数に渡された`opts`テーブルは、全てのプラグインのデフォルト設定と`Localrc`マージされます。

以下は設定可能な全オプションとデフォルト値です。

```lua
opts = {
  -- UIバックエンドに関する設定
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

  -- ログ出力に関する設定
  logging = {
    level = "info", -- 全体の基本ログレベル (trace, debug, info, warn, error)
    echo = { level = "warn" }, -- :echoで表示する最低レベル
    notify = { level = "error", prefix = "[UNL]" }, -- vim.notifyで表示する最低レベルと接頭辞
    file = { enable = true, max_kb = 512, rotate = 3, filename = "unl.log" }, -- ファイルログ設定
  },

  -- キャッシュディレクトリに関する設定
  cache = {
    -- このライブラリや関連プラグインがキャッシュを保存する
    -- <nvim_cache_dir>/<dirname> というディレクトリ名
    dirname = "UNL_cache"
  },

  -- プロジェクト探索に関する設定
  project = {
    -- プロジェクトローカルな設定ファイル名
    localrc_filename = ".unlrc.json",
    -- trueの場合、ホームディレクトリより上は探索しない
    search_stop_at_home = true,
  },
}
```

## 🤖 プラグイン開発者向け (For Plugin Developers)

`UNL.nvim`を利用することで、プラグイン開発を大幅に簡略化できます。

### 基本的な使い方

```lua
-- my_plugin/init.lua

-- UNLのコアモジュールをインポート
local unl_log = require("UNL.logging")
local unl_config = require("UNL.config")
-- 自分のプラグインのデフォルト設定を定義
local my_defaults = require("my_plugin.config.defaults")

local M = {}

function M.setup(user_config)
  -- 1. UNLのシステムに自分のプラグインを登録
  -- これでロガーと設定が初期化される
  unl_log.setup("MyPlugin", my_defaults, user_config or {})

  -- 2. これ以降、自分のプラグイン用のロガーや設定が取得できる
  local log = unl_log.get("MyPlugin")
  local conf = unl_config.get("MyPlugin")

  log.info("MyPlugin has been set up successfully!")
  log.debug("Current picker mode is: %s", conf.ui.picker.mode)
end

return M
```

## 📜 ライセンス (License)

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
