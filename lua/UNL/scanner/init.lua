local M = {}

--- スキャナバイナリのパスを取得する (純粋なゲッター)
-- @return string|nil バイナリの絶対パス。見つからない場合はnil
function M.get_binary_path()
    local plugin_root = vim.fn.fnamemodify(debug.getinfo(1).source:sub(2), ":h:h:h:h")
    local is_win = vim.loop.os_uname().version:find("Windows") or vim.fn.has("win32") == 1
    local binary_name = is_win and "unl-scanner.exe" or "unl-scanner"
    
    -- 1. releaseビルドを優先
    local release_path = plugin_root .. "/scanner/target/release/" .. binary_name
    if vim.loop.fs_stat(release_path) then
        return release_path
    end
    
    -- 2. debugビルドをフォールバック
    local debug_path = plugin_root .. "/scanner/target/debug/" .. binary_name
    if vim.loop.fs_stat(debug_path) then
        return debug_path
    end
    
    return nil
end

--- バイナリが存在するか確認する
function M.has_binary()
    return M.get_binary_path() ~= nil
end

--- バイナリが見つからない場合の警告を表示する
function M.warn_binary_missing()
    local log = require("UNL.logging").get("UNL")
    log.warn_once(
        "Scanner binary not found. Please build it by running: cargo build --release --manifest-path scanner/Cargo.toml\n" ..
        "Or add { 'taku25/UNL.nvim', build = 'cargo build --release --manifest-path scanner/Cargo.toml' } to your plugin spec."
    )
end

--- スキャナを実行する (非同期)
-- @param payload table ファイルリスト [{path, mtime, old_hash}]
-- @param on_result function(result_table) 各ファイルの結果が返るたびに呼ばれる
-- @param on_complete function(ok, err) 全ての処理が終わった時に呼ばれる
function M.run_async(payload, on_result, on_complete)
    local binary = M.get_binary_path()
    if not binary then
        M.warn_binary_missing()
        if on_complete then on_complete(false, "Scanner binary not found.") end
        return nil
    end

    local input_json = vim.json.encode(payload)
    local job_id = vim.fn.jobstart({ binary }, {
        stdout_buffered = false,
        on_stdout = function(_, data)
            if not data then return end
            for _, line in ipairs(data) do
                if line ~= "" then
                    local ok, res = pcall(vim.json.decode, line)
                    if ok and on_result then
                        on_result(res)
                    end
                end
            end
        end,
        on_stderr = function(_, data)
            if data then
                local log = require("UNL.logging").get("UNL")
                for _, line in ipairs(data) do
                    if line ~= "" then
                        log.error("[Scanner Error] %s", line)
                    end
                end
            end
        end,
        on_exit = function(_, code)
            if on_complete then
                on_complete(code == 0)
            end
        end
    })

    if job_id > 0 then
        vim.fn.chansend(job_id, input_json)
        vim.fn.chanclose(job_id, "stdin")
    else
        if on_complete then on_complete(false, "Failed to start scanner process.") end
    end

    return job_id
end

return M