-- lua/UNL/cmd/delete.lua
local rpc = require("UNL.rpc")
local registry = require("UNL.registry")
local unl_picker = require("UNL.backend.picker")
local unl_config = require("UNL.config")
local log = require("UNL.logging").get("UNL")
local unl_events = require("UNL.event.events")
local unl_event_types = require("UNL.event.types")

local M = {}

local function execute_deletion(project_root, display_name)
    local prompt_str = ("Permanently remove '%s' from the UNL registry?"):format(display_name)
    local choices = "&Yes\n&No"
    local decision = vim.fn.confirm(prompt_str, choices, 2)

    if decision ~= 1 then
        log.info("Deletion cancelled.")
        return
    end

    rpc.request("delete_project", { project_root = project_root }, nil, function(success, result)
        if success then
            log.info("Project removed from registry: %s", display_name)
            unl_events.publish(unl_event_types.ON_AFTER_DELETE_PROJECT_REGISTRY, {
                status = "success",
                project_root = project_root,
            })
        else
            log.warn("RPC deletion failed, attempting local removal.")
            if registry.remove(project_root) then
                log.info("Project removed from local registry: %s", display_name)
                unl_events.publish(unl_event_types.ON_AFTER_DELETE_PROJECT_REGISTRY, { status = "success", project_root = project_root })
            else
                log.error("Failed to remove project locally.")
            end
        end
    end)
end

function M.execute(opts)
  log.debug("Fetching registered projects for deletion...")
  
  local function show_picker(projects)
      if #projects == 0 then
          log.warn("No registered projects found.")
          return
      end

      local picker_items = {}
      for _, p in ipairs(projects) do
          local root = p.root
          local display_name = vim.fn.fnamemodify(root, ":t")
          table.insert(picker_items, {
              label = string.format("%s (%s)", display_name, root),
              value = root,
              display_name = display_name
          })
      end

      table.sort(picker_items, function(a, b) return a.label < b.label end)

      unl_picker.pick({
          kind = "unl_project_delete",
          title = "Select Project to DELETE from UNL registry",
          items = picker_items,
          conf = unl_config.get("UNL"),
          preview_enabled = false,
          on_submit = function(selected_root, item)
              if not selected_root then return end
              execute_deletion(selected_root, item.display_name or selected_root)
          end,
      })
  end

  rpc.request("list_projects", {}, nil, function(success, projects)
    if success and type(projects) == "table" then
        show_picker(projects)
    else
        log.warn("RPC list_projects failed, falling back to local registry.")
        local local_list = registry.load()
        show_picker(local_list)
    end
  end)
end

return M
