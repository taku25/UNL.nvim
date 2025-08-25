-- lua/UNL/command/builder.lua (完全版)

local M = {}

function M.create(spec)
  -- 0. 依存関係チェック
  local function get_logger()
    -- specから渡された "UEP" という名前を使って、ロガーマネージャーに問い合わせる
    return require("UNL.logging").get(spec.plugin_name or "UNL")
  end

  if spec.dependencies then
    for _, dep in ipairs(spec.dependencies) do
      local ok, result = pcall(dep.check)
      if not (ok and result) then
        local error_msg = ("%s requires '%s'. %s"):format(spec.cmd_name, dep.name, dep.msg or "")
        vim.api.nvim_echo({ { error_msg, "ErrorMsg" } }, true, { err = true })
        return
      end
    end
  end

  -- 1. バージョンチェック
  if spec.version and 1 ~= vim.fn.has(spec.version) then
    local error_msg = ("%s requires at least nvim-%s"):format(spec.cmd_name, spec.version)
    vim.api.nvim_echo({ { error_msg, "ErrorMsg" } }, true, { err = true })
    return
  end

  -- 2. ロードガード
  local guard_key = "loaded_" .. spec.cmd_name:lower()
  if vim.g[guard_key] then return end
  vim.g[guard_key] = true

  -- ★★★ 変更点: ロガーを遅延束縛する ★★★

  -- 3. コマンドハンドラ本体
  local function command_handler(args)
    -- 3a. サブコマンドが指定されていない場合は使い方を表示
    if not args.fargs or #args.fargs == 0 then
      get_logger().warn("Usage: :" .. spec.cmd_name .. " <subcommand> ...")
      return
    end

    -- 3b. bang(!)の有無を確定させる
    local has_bang = (args.bang == "!") or (args.fargs[1] and args.fargs[1]:match("!$") ~= nil)

    -- 3c. サブコマンド名を安全に抽出する
    local sub_name_raw = args.fargs[1]
    local sub_name = sub_name_raw:gsub("!$", "")

    -- 3d. サブコマンド定義を検索
    local command_def = spec.subcommands[sub_name:lower()]
    if not command_def then
      get_logger().error("Unknown subcommand: " .. sub_name_raw)
      return
    end

    -- 3e. bang(!)のサポートをチェック
    if has_bang and not command_def.bang then
      get_logger().error(("Subcommand '%s' does not support bang (!)."):format(sub_name))
      return
    end

    -- 3f. ハンドラに渡す `opts` テーブルを準備
    local opts = { has_bang = has_bang }
    local user_args = {}
    for i = 2, #args.fargs do
      table.insert(user_args, args.fargs[i])
    end

    -- 3g. 定義に基づいて引数をパース
    if command_def.args then
      for i, arg_def in ipairs(command_def.args) do
        local value = user_args[i]
        if value == nil then
          if arg_def.required then
            get_logger().error("Missing required argument: '%s'. Usage: %s", arg_def.name, command_def.usage or "")
            return
          end
          if type(arg_def.default) == "function" then
            value = arg_def.default()
          else
            value = arg_def.default
          end
        end
        opts[arg_def.name] = value
      end
    end
    
    -- 3h. 実行前フックを呼び出す
    if spec.on_before_command then
      pcall(spec.on_before_command, { subcommand = sub_name, opts = opts })
    end
    
    -- 3i. 最終的なハンドラを実行
    command_def.handler(opts)
  end

  -- 4. 補完ハンドラ
  local function complete_handler(arglead, cmdline, cursorpos)
    local parts = vim.split(cmdline, " ", true)
    if #parts <= 2 then
      return vim.tbl_filter(function(name)
        return vim.startswith(name, arglead)
      end, vim.tbl_keys(spec.subcommands))
    end
    return {}
  end

  -- 5. コマンド登録
  vim.api.nvim_create_user_command(spec.cmd_name, command_handler, {
    nargs = "*",
    bang = true,
    desc = spec.desc or (spec.cmd_name .. " commands"),
    complete = complete_handler,
  })

  get_logger().info("%s commands registered.", spec.cmd_name)
end

return M
