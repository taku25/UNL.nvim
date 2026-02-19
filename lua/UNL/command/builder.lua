-- lua/UNL/command/builder.lua (完全版)

local M = {}

function M.create(spec)
  -- 0. 依存関係とロガーの準備
  local function get_logger()
    return require("UNL.logging").get(spec.plugin_name or "UNL")
  end

  if spec.dependencies then
    for _, dep in ipairs(spec.dependencies) do
      local ok, result = pcall(dep.check)
      if not (ok and result) then
        local error_msg = ("%s requires '%s'. %s"):format(spec.cmd_name, dep.name, dep.msg or "")
        get_logger().error(error_msg)
        return
      end
    end
  end

  -- 1. ロードガード (二重読み込み防止)
  local guard_key = "loaded_cmd_" .. spec.cmd_name:lower()
  if vim.g[guard_key] then
    return
  end
  vim.g[guard_key] = true

  -- 2. コマンドハンドラ本体
  local function command_handler(args)
    -- args = { fargs, bang, ... }

    -- 2a. サブコマンドが指定されていない場合は使い方を表示
    if not args.fargs or #args.fargs == 0 then
      get_logger().warn("Usage: :" .. spec.cmd_name .. " <subcommand> ...")
      return
    end

    -- 2b. bang(!)の有無を確定させる
    local has_bang = (args.bang == "!") or (args.fargs[1] and args.fargs[1]:match("!$") ~= nil)

    -- 2c. サブコマンド名を安全に抽出する
    local sub_name_raw = args.fargs[1]
    local sub_name = sub_name_raw:gsub("!$", "")

    -- 2d. サブコマンド定義を検索
    local command_def = spec.subcommands[sub_name:lower()]
    if not command_def then
      get_logger().error("Unknown subcommand: " .. sub_name_raw)
      return
    end

    -- 2e. コマンド定義がbang(!)をサポートしているかチェック
    if has_bang and not command_def.bang then
      get_logger().error(("Subcommand '%s' does not support bang (!)."):format(sub_name))
      return
    end

    -- 2f. ハンドラに渡す `opts` テーブルを準備
    local opts = { has_bang = has_bang }
    -- ▼▼▼ ここからが修正箇所です ▼▼▼

    -- 2g. ユーザー引数を「位置引数」「フラグ引数」「名前付き引数」に分類する
    local positional_args = {}
    local flag_args = {}
    local named_args = {}
    local var_args = {}
    for i = 2, #args.fargs do
      local arg = args.fargs[i]
      local named_key, _ = next(named_args)
      if arg:sub(1, 2) == "--" then
        if arg:match("^--[%w_]+=") then
          local key, val = arg:match("^--([%w_]+)=(.*)$")
          if flag_args[key] then
            get_logger().error("Multiple definitions of flag: %s.", key)
            return
          end
          flag_args[key] = val
        else
          if flag_args[arg:sub(3)] then
            get_logger().error("Multiple definitions of flag: %s.", arg:sub(3))
            return
          end
          flag_args[arg:sub(3)] = true
        end
      elseif arg:match("^[%w_]+=") then
        local key, val = arg:match("^([%w_]+)=(.*)$")
        if named_args[key] then
          get_logger().error("Multiple definitions of argument: %s.", named_args[key])
          return
        end
        named_args[key] = val
      elseif named_key ~= nil then
        table.insert(var_args, arg)
      else
        table.insert(positional_args, arg)
      end
    end

    -- 2h. 定義に基づいて引数をパース
    if command_def.args then
      local positional_idx = 1
      local has_variadic = nil
      for _, arg_def in ipairs(command_def.args) do
        if arg_def.name:match("_flag$") then
          for flag_key, flag_value in pairs(flag_args) do
            if arg_def.name:match("^([%w_]+)_flag$") == flag_key then
              if opts[arg_def.name] then
                get_logger().error("Flag '%s' was already set.", flag_key)
                return
              end
              opts[arg_def.name] = flag_value
              flag_args[flag_key] = nil
              break
            end
          end
          if opts[arg_def.name] == nil and arg_def.default ~= nil then
            opts[arg_def.name] = (type(arg_def.default) == "function") and arg_def.default()
              or arg_def.default
          end
        elseif arg_def.variadic then
          if has_variadic ~= nil then
            get_logger().error(
              "Only one variadic argument is allowed, please contact the plugin maintainer."
            )
            return
          end
          if named_args[arg_def.name] then
            if opts[arg_def.name] then
              get_logger().error("Argument '%s' was already set.", arg_def.name)
              return
            end
            local vals = {}
            for v in string.gmatch(named_args[arg_def.name], "([^,]+)") do
              table.insert(vals, v)
            end
            opts[arg_def.name] = vals
            named_args[arg_def.name] = nil
          end
          has_variadic = arg_def
        elseif positional_idx <= #positional_args then
          opts[arg_def.name] = positional_args[positional_idx]
          positional_idx = positional_idx + 1
        else
          if named_args[arg_def.name] then
            if opts[arg_def.name] then
              get_logger().error("Argument '%s' was already set.", arg_def.name)
              return
            end
            opts[arg_def.name] = named_args[arg_def.name]
            named_args[arg_def.name] = nil
          elseif arg_def.required then
            get_logger().error(
              "Missing required argument: '%s'. Usage: %s",
              arg_def.name,
              command_def.desc or ""
            )
            return
          elseif opts[arg_def.name] == nil and arg_def.default ~= nil then
            opts[arg_def.name] = (type(arg_def.default) == "function") and arg_def.default()
              or arg_def.default
          end
        end
      end
      local flag_key, _ = next(flag_args)
      if flag_key ~= nil then
        local flags = {}
        for key, _ in pairs(flag_args) do
          table.insert(flags, key)
        end
        get_logger().error("Unused flags: %s", table.concat(flags, ", "))
        return
      end
      local named_key, _ = next(named_args)
      if named_key ~= nil then
        local named = {}
        for key, _ in pairs(named_args) do
          table.insert(named, key)
        end
        get_logger().error("Unused named arguments: %s", table.concat(named, ", "))
        return
      end
      if #positional_args >= positional_idx or #var_args > 0 then
        if has_variadic == nil then
          get_logger().error("This command does not allow variadic arguments")
          return
        elseif opts[has_variadic.name] then
          get_logger().error("Variadic arguments already set by using a named argument.")
          return
        end
        local varia_vals = {}
        for i = positional_idx, #positional_args do
          table.insert(varia_vals, positional_args[i])
        end
        for _, varia_val in ipairs(var_args) do
          table.insert(varia_vals, varia_val)
        end
        opts[has_variadic.name] = varia_vals
      end
      if has_variadic and has_variadic.required and opts[has_variadic.name] == nil then
        get_logger().error(
          "Missing required variadic argument: '%s'. Usage: %s",
          has_variadic.name,
          command_def.desc or ""
        )
        return
      end
      if has_variadic and opts[has_variadic.name] == nil and has_variadic.default ~= nil then
        if type(has_variadic.default) == "function" then
          opts[has_variadic.name] = has_variadic.default()
        else
          local vals = {}
          for v in string.gmatch(has_variadic.default, "([^,]+)") do
            table.insert(vals, v)
          end
          opts[has_variadic.name] = vals
        end
      end
    else
      opts["args"] = { unpack(args.fargs, 2) }
    end

    -- ▲▲▲ ここまでが修正箇所です ▲▲▲

    -- 2i. 最終的なハンドラを実行
    command_def.handler(opts)
  end

  -- 3. 補完ハンドラ
  local function complete_handler(arglead, cmdline, cursorpos)
    local parts = vim.split(cmdline, " ", true)
    -- サブコマンド名を補完
    if #parts <= 2 then
      return vim.tbl_filter(function(name)
        return vim.startswith(name, arglead)
      end, vim.tbl_keys(spec.subcommands))
    end
    -- (将来的に各サブコマンドの引数補完もここに追加可能)
    return {}
  end

  -- 4. コマンド登録
  vim.api.nvim_create_user_command(spec.cmd_name, command_handler, {
    nargs = "*",
    bang = true,
    desc = spec.desc or (spec.cmd_name .. " commands"),
    complete = complete_handler,
  })

  get_logger().debug("%s commands registered.", spec.cmd_name)
end

return M
