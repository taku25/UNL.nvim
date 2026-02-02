local M = {}
local finder = require("UNL.finder")
local remote = require("UNL.db.remote")
local log = require("UNL.logging").get("UNL")

function M.get_maps(start_path, on_complete)
  log.debug("project.get_maps (RPC) called...")
  local start_time = os.clock()

  local project_root = finder.project.find_project_root(start_path)
  if not project_root then
    log.error("project.get_maps: Could not find project root.")
    return on_complete(false, "Could not find project root.")
  end

  -- 1. Get Components
  remote.get_components(function(components, err1)
    if err1 then
      log.error("project.get_maps: Failed to get components via RPC: %s", tostring(err1))
      return on_complete(false, err1)
    end
    
    if type(components) ~= "table" or #components == 0 then
      log.warn("project.get_maps: No components in DB. Run :UNL refresh.")
      return on_complete(false, "No components in DB.")
    end

    -- 2. Get Modules
    remote.get_modules(function(modules_rows, err2)
      if err2 then
        log.error("project.get_maps: Failed to get modules via RPC: %s", tostring(err2))
        return on_complete(false, err2)
      end

      local all_components_map = {}
      local all_modules_map = {}
      local module_to_component_name = {}
      local runtime_modules_map, developer_modules_map, editor_modules_map, programs_modules_map = {}, {}, {}, {}
      local game_name, engine_name

      for _, comp in ipairs(components) do
        all_components_map[comp.name] = {
          name = comp.name,
          display_name = comp.display_name,
          type = comp.type,
          owner_name = comp.owner_name,
          root_path = comp.root_path,
          uplugin_path = comp.uplugin_path,
          uproject_path = comp.uproject_path,
          engine_association = comp.engine_association,
        }
        if comp.type == "Game" then game_name = comp.name end
        if comp.type == "Engine" then engine_name = comp.name end
      end

      for _, row in ipairs(modules_rows or {}) do
        local deep_deps = nil
        if row.deep_dependencies and row.deep_dependencies ~= "" then
            -- row.deep_dependencies が userdata の可能性があるため、tostring または明示的変換が必要
            local raw_deps = tostring(row.deep_dependencies)
            local ok, res = pcall(vim.json.decode, raw_deps)
            if ok then deep_deps = res end
        end

        local mod_meta = {
          name = tostring(row.name),
          type = tostring(row.type or ""),
          scope = tostring(row.scope or ""),
          module_root = tostring(row.root_path),
          path = row.build_cs_path and tostring(row.build_cs_path) or nil,
          owner_name = tostring(row.owner_name or ""),
          component_name = tostring(row.component_name or ""),
          deep_dependencies = deep_deps,
        }

        all_modules_map[row.name] = mod_meta
        module_to_component_name[row.name] = row.component_name

        local t = (row.type or ""):lower()
        if t == "program" then programs_modules_map[row.name] = mod_meta
        elseif t == "developer" then developer_modules_map[row.name] = mod_meta
        elseif t:find("editor", 1, true) or t == "uncookedonly" then editor_modules_map[row.name] = mod_meta
        else runtime_modules_map[row.name] = mod_meta end
      end

      local end_time = os.clock()
      log.debug("project.get_maps finished in %.4f seconds (RPC). Found %d modules across %d components.",
                end_time - start_time, vim.tbl_count(all_modules_map), vim.tbl_count(all_components_map))

      local engine_root = engine_name and all_components_map[engine_name] and all_components_map[engine_name].root_path

      on_complete(true, {
        project_root = project_root,
        engine_root = engine_root,
        all_modules_map = all_modules_map,
        module_to_component_name = module_to_component_name,
        all_components_map = all_components_map,
        runtime_modules_map = runtime_modules_map,
        developer_modules_map = developer_modules_map,
        editor_modules_map = editor_modules_map,
        programs_modules_map = programs_modules_map,
        game_component_name = game_name,
        engine_component_name = engine_name,
      })
    end)
  end)
end

--- 指定されたスコープに該当するモジュール名のリストを取得
function M.get_modules_by_scope(opts, callback)
    opts = opts or {}
    local requested_scope = (opts.scope and opts.scope:lower()) or "runtime"
    local deps_flag = opts.deps_flag or "--deep-deps"

    M.get_maps(vim.loop.cwd(), function(ok, maps)
        if not ok then return callback(nil, maps) end

        local function path_under_root(path, root)
            if not path or not root then return false end
            local p = path:gsub("\\", "/"):lower()
            local r = root:gsub("\\", "/"):lower()
            if not r:match("/$") then r = r .. "/" end
            return p:sub(1, #r) == r
        end

        local seed_modules = {}
        local game_name = maps.game_component_name
        local engine_name = maps.engine_component_name
        local game_root = (maps.all_components_map[game_name] or {}).root_path
        local engine_root = (maps.all_components_map[engine_name] or {}).root_path

        for name, m in pairs(maps.all_modules_map) do
            local is_owner_match = false
            if requested_scope == "game" then
                is_owner_match = (m.owner_name == game_name) or path_under_root(m.module_root, game_root)
            elseif requested_scope == "engine" then
                is_owner_match = (m.owner_name == engine_name) or path_under_root(m.module_root, engine_root)
            elseif requested_scope == "runtime" then
                is_owner_match = (m.type == "Runtime") or (m.owner_name == game_name)
            elseif requested_scope == "editor" then
                local t = (m.type or ""):lower()
                is_owner_match = (t == "runtime" or t == "developer" or t:find("editor", 1, true) or t == "uncookedonly")
            elseif requested_scope == "full" then
                is_owner_match = (m.type ~= "Program")
            end

            if is_owner_match then seed_modules[name] = true end
        end

        local target_module_names = seed_modules
        if deps_flag ~= "--no-deps" then
            local deps_key = (deps_flag == "--deep-deps") and "deep_dependencies" or "shallow_dependencies"
            for mod_name, _ in pairs(seed_modules) do
                local mod_meta = maps.all_modules_map[mod_name]
                if mod_meta and mod_meta[deps_key] then
                    for _, dep_name in ipairs(mod_meta[deps_key]) do
                        target_module_names[dep_name] = true
                    end
                end
            end
        end

        callback(vim.tbl_keys(target_module_names), maps)
    end)
end

return M
