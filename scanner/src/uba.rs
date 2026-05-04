// uba.rs — UBA (Unreal Build Accelerator) binary trace file parser
//
// Format confirmed from UE5.7 source:
//   UbaTrace.h, UbaTraceReader.h, UbaBinaryReaderWriter.h, UbaProcessHandle.h
//
// File layout:
//   [0-3]  fileSize: u32 LE
//   [4-7]  version:  u32 LE  (TraceVersion = 49)
//   [8..]  header fields (variable, scanned heuristically)
//   [N..]  records: [type:u8][timestamp:7bit_u64][data...]
//
// 7-bit encoding (BinaryWriter::Write7BitEncoded):
//   LSB-first variable-length; high bit = "more bytes follow"
//   e.g. 0xF1 0x24 → (0x71) | (0x24 << 7) = 113 + 4608 = 4721

use std::fs;
use std::path::Path;

// ─────────────────────────────────────────────────────────────────
// Trace type constants
// ─────────────────────────────────────────────────────────────────
const T_SESSION_ADDED: u8        = 0;
const T_SESSION_UPDATE: u8       = 1;
const T_PROCESS_ADDED: u8        = 2;
const T_PROCESS_EXITED: u8       = 3;
const T_PROCESS_RETURNED: u8     = 4;
const T_FILE_FETCH_BEGIN: u8     = 5;
const T_FILE_FETCH_END: u8       = 6;
const T_FILE_STORE_BEGIN: u8     = 7;
const T_FILE_STORE_END: u8       = 8;
const T_SUMMARY: u8              = 9;
const T_WORK_BEGIN: u8           = 10;
const T_WORK_END: u8             = 11;
const T_STRING_RECORD: u8        = 12;
const T_SESSION_SUMMARY: u8      = 13;
const T_PROCESS_ENV_UPDATED: u8  = 14;
const T_SESSION_DISCONNECT: u8   = 15;
const T_PROXY_CREATED: u8        = 16;
const T_PROXY_USED: u8           = 17;
const T_FILE_FETCH_LIGHT: u8     = 18;
const T_FILE_STORE_LIGHT: u8     = 19;
const T_STATUS_UPDATE: u8        = 20;
const T_SESSION_NOTIFICATION: u8 = 21;
const T_CACHE_BEGIN_FETCH: u8    = 22;
const T_CACHE_END_FETCH: u8      = 23;
const T_CACHE_BEGIN_WRITE: u8    = 24;
const T_CACHE_END_WRITE: u8      = 25;
const T_PROGRESS_UPDATE: u8      = 26;
const T_REMOTE_EXEC_DISABLED: u8 = 27;
const T_FILE_FETCH_SIZE: u8      = 28;
const T_PROCESS_BREADCRUMBS: u8  = 29;
const T_WORK_HINT: u8            = 30;
const T_DRIVE_UPDATE: u8         = 31;
const T_CACHE_SUMMARY: u8        = 32;
const T_TASK_BEGIN: u8           = 33;
const T_TASK_HINT: u8            = 34;
const T_TASK_END: u8             = 35;
const T_SCHEDULER_UPDATE: u8     = 36;
const T_SCHEDULER_KILL: u8       = 37;
const T_SESSION_INFO: u8         = 38;
const T_MAX_VALID: u8            = 38;

// ─────────────────────────────────────────────────────────────────
// Byte-level reader
// ─────────────────────────────────────────────────────────────────
struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }

    fn remaining(&self) -> usize { self.data.len().saturating_sub(self.pos) }

    fn read_u8(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() { return None; }
        let v = self.data[self.pos];
        self.pos += 1;
        Some(v)
    }

    fn read_u32_le(&mut self) -> Option<u32> {
        if self.pos + 4 > self.data.len() { return None; }
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().ok()?);
        self.pos += 4;
        Some(v)
    }

    /// 7-bit variable-length little-endian unsigned integer
    fn read_7bit(&mut self) -> Option<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            if self.pos >= self.data.len() || shift > 63 { return None; }
            let b = self.data[self.pos];
            self.pos += 1;
            value |= ((b & 0x7F) as u64) << shift;
            shift += 7;
            if b & 0x80 == 0 { break; }
        }
        Some(value)
    }

    /// Length-prefixed UTF-8 string (length encoded as 7-bit)
    fn read_string(&mut self) -> Option<String> {
        let len = self.read_7bit()? as usize;
        if len > 4096 { return None; } // sanity: no single string > 4 KB
        if self.pos + len > self.data.len() { return None; }
        let s = String::from_utf8_lossy(&self.data[self.pos..self.pos + len]).into_owned();
        self.pos += len;
        Some(s)
    }

    fn skip(&mut self, n: usize) -> bool {
        if self.pos + n > self.data.len() { return false; }
        self.pos += n;
        true
    }
}

// ─────────────────────────────────────────────────────────────────
// Timestamp formatting (QPC-based, assume 10 MHz default)
// ─────────────────────────────────────────────────────────────────
fn fmt_ts(ts: u64, freq: u64) -> String {
    if freq == 0 || ts == 0 { return "00:00:00.000".to_string(); }
    let ms  = ts * 1_000 / freq;
    let s   = ms  / 1_000;
    let m   = s   / 60;
    let h   = m   / 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m % 60, s % 60, ms % 1_000)
}

// ─────────────────────────────────────────────────────────────────
// Heuristic: find where records start
// ─────────────────────────────────────────────────────────────────
/// Try to parse N records at `from`; return how many succeeded.
fn probe_at(data: &[u8], from: usize) -> usize {
    let mut r = Reader::new(data);
    r.pos = from;
    let mut count = 0usize;

    for _ in 0..8 {
        if r.remaining() == 0 { break; }
        let save = r.pos;

        let type_byte = match r.read_u8() { Some(b) => b, None => break };
        if type_byte > T_MAX_VALID { r.pos = save; break; }

        let _ts = match r.read_7bit() { Some(v) => v, None => { r.pos = save; break; } };

        let ok = match type_byte {
            T_SESSION_INFO | T_SESSION_NOTIFICATION => {
                r.read_u32_le().is_some() && r.read_string().is_some()
            }
            T_SESSION_ADDED => {
                r.read_u32_le().is_some() && r.read_u32_le().is_some()
                    && r.read_string().is_some() && r.read_string().is_some()
            }
            T_SESSION_SUMMARY => {
                if r.read_u32_le().is_none() { false }
                else {
                    let n = match r.read_7bit() { Some(v) => v as usize, None => { break; } };
                    let mut ok2 = true;
                    for _ in 0..n.min(64) {
                        if r.read_string().is_none() || r.read_string().is_none() {
                            ok2 = false; break;
                        }
                    }
                    ok2
                }
            }
            T_PROCESS_ADDED => {
                if r.read_u32_le().is_none() || r.read_u32_le().is_none() { false }
                else {
                    let desc = r.read_string();
                    let bcr  = r.read_string();
                    let pt   = r.read_u8();
                    desc.as_ref().map(|s| s.len() <= 256).unwrap_or(false)
                        && bcr.is_some() && pt.is_some()
                }
            }
            T_PROGRESS_UPDATE => {
                r.read_7bit().is_some() && r.read_7bit().is_some() && r.read_7bit().is_some()
            }
            // For everything else: stop probing (can't safely parse, but count what we have)
            _ => { break; }
        };

        if !ok { r.pos = save; break; }
        count += 1;
    }
    count
}

fn find_records_start(data: &[u8]) -> usize {
    let limit = data.len().min(512);

    // Strategy 1: Find the first T_SESSION_INFO (38) followed by a valid
    // timestamp + small sessionId + non-empty ASCII string.
    // SessionInfo is always among the first records in a UBA trace.
    for start in 8..limit {
        if data[start] != T_SESSION_INFO { continue; }
        let mut r = Reader::new(data);
        r.pos = start + 1;
        let _ts = match r.read_7bit() { Some(v) => v, None => continue };
        let sid = match r.read_u32_le() { Some(v) => v, None => continue };
        if sid >= 1_000 { continue; } // session IDs are tiny
        let text = match r.read_string() { Some(s) => s, None => continue };
        if text.len() >= 4 && text.is_ascii() {
            return start;
        }
    }

    // Strategy 2: Generic probe — find offset with ≥3 valid consecutive records
    for start in 8..limit {
        if probe_at(data, start) >= 3 {
            return start;
        }
    }

    8 // fallback
}

// ─────────────────────────────────────────────────────────────────
// Public output structure
// ─────────────────────────────────────────────────────────────────
pub struct UbaParseResult {
    pub version: u32,
    pub lines:   Vec<String>,
}

// ─────────────────────────────────────────────────────────────────
// Main parser
// ─────────────────────────────────────────────────────────────────
pub fn parse_uba_file<P: AsRef<Path>>(path: P) -> anyhow::Result<UbaParseResult> {
    let data = fs::read(path.as_ref())?;
    if data.len() < 8 {
        return Err(anyhow::anyhow!("File too small"));
    }

    let _file_size = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let version   = u32::from_le_bytes(data[4..8].try_into().unwrap());

    let fname = path.as_ref().file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown");

    let mut out: Vec<String> = Vec::new();
    let bar = "─".repeat(54);
    out.push(format!("┌{}┐", bar));
    out.push(format!("│  UBA Build Trace  │  version={}  │  {:<20}│", version, fname));
    out.push(format!("└{}┘", bar));
    out.push(String::new());

    // Typical Windows QPC frequency = 10 MHz; UBA may store frequency in the header.
    // Without reading it explicitly, use 10 MHz as default.
    let freq: u64 = 10_000_000;

    let start = find_records_start(&data);
    let mut r = Reader::new(&data);
    r.pos = start;

    // Map processId → description for later cross-reference in ProcessExited
    let mut process_names: std::collections::HashMap<u32, String> = Default::default();

    let mut consec_fail = 0usize;
    let max_fail = 512;
    let mut last_nonzero_ts: u64 = 0; // for retrograde-timestamp detection

    while r.remaining() > 0 && consec_fail < max_fail {
        let rec_start = r.pos;
        let type_byte = match r.read_u8() { Some(b) => b, None => break };

        if type_byte > T_MAX_VALID {
            consec_fail += 1;
            continue;
        }
        consec_fail = 0;

        let ts = match r.read_7bit() { Some(v) => v, None => break };

        // Retrograde-timestamp detection: if ts is 0 but we've already seen real time,
        // this record is likely parsed from a binary blob. Suppress its output.
        let suspicious = ts == 0 && last_nonzero_ts > 1_000_000;
        if ts > last_nonzero_ts { last_nonzero_ts = ts; }
        let ts_str = fmt_ts(ts, freq);

        match type_byte {
            // ── Well-understood records ──────────────────────────────────────
            T_SESSION_INFO => {
                let Some(_sid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(text) = r.read_string()  else { r.pos = rec_start + 1; continue };
                if !suspicious && text.is_ascii() {
                    out.push(format!("[{}] SESSION   {}", ts_str, text));
                }
            }

            T_SESSION_ADDED => {
                let Some(_sid)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(_cid)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(name)  = r.read_string() else { r.pos = rec_start + 1; continue };
                let Some(_mid)  = r.read_string() else { r.pos = rec_start + 1; continue };
                // Filter: stats blobs often masquerade as SESSION_ADDED with apostrophes or
                // padded spaces (e.g., "  Time    243ms'  Exit Time...")
                let clean = !name.is_empty() && name.is_ascii()
                    && !name.contains('\'')
                    && !name.contains("  ");
                if !suspicious && clean {
                    out.push(format!("[{}] CONNECTED {}", ts_str, name));
                }
            }

            T_SESSION_DISCONNECT => {
                let Some(_sid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                if !suspicious {
                    out.push(format!("[{}] DISCONNECT", ts_str));
                }
            }

            T_SESSION_NOTIFICATION => {
                let Some(_sid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(text) = r.read_string() else { r.pos = rec_start + 1; continue };
                if !suspicious && !text.is_empty() && text.is_ascii() {
                    out.push(format!("[{}] NOTIFY    {}", ts_str, text));
                }
            }

            T_SESSION_SUMMARY => {
                let Some(_sid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(n)    = r.read_7bit()   else { r.pos = rec_start + 1; continue };
                out.push(format!("[{}] ── BUILD SUMMARY ──", ts_str));
                let mut ok = true;
                for _ in 0..n.min(128) {
                    let name  = match r.read_string() { Some(s) => s, None => { ok = false; break } };
                    let value = match r.read_string() { Some(s) => s, None => { ok = false; break } };
                    if !name.is_empty() {
                        out.push(format!("               {:<24} {}", name, value));
                    }
                }
                if !ok { r.pos = rec_start + 1; }
            }

            T_PROCESS_ADDED => {
                let Some(_sid) = r.read_u32_le()  else { r.pos = rec_start + 1; continue };
                let Some(pid)  = r.read_u32_le()  else { r.pos = rec_start + 1; continue };
                let Some(desc) = r.read_string()  else { r.pos = rec_start + 1; continue };
                let Some(_bcr) = r.read_string()  else { r.pos = rec_start + 1; continue };
                let Some(_pt)  = r.read_u8()      else { r.pos = rec_start + 1; continue };
                // Sanity: real PIDs stay under 10 million; descriptions must be ASCII
                if !suspicious && pid < 10_000_000 && !desc.is_empty() && desc.is_ascii() {
                    process_names.insert(pid, desc.clone());
                    out.push(format!("[{}] COMPILE   #{:<5} {}", ts_str, pid, desc));
                }
            }

            T_PROCESS_BREADCRUMBS => {
                // processId(u32) + breadcrumbs(string)
                let Some(_pid)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(_bcr)  = r.read_string() else { r.pos = rec_start + 1; continue };
            }

            T_PROCESS_RETURNED => {
                // processId(u32) + (same client, lightweight) — no extra fields known
                let Some(_pid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
            }

            T_PROCESS_EXITED => {
                // processId(u32) + exitCode(u32), then stats blob of unknown size + log lines
                let Some(pid)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(exit) = r.read_u32_le() else { r.pos = rec_start + 1; continue };

                let desc = process_names.get(&pid).cloned()
                    .unwrap_or_else(|| format!("process #{}", pid));
                if !suspicious && pid < 10_000_000 {
                    if exit == 0 {
                        out.push(format!("[{}] DONE      #{:<5} {} → OK", ts_str, pid, desc));
                    } else {
                        out.push(format!("[{}] FAILED    #{:<5} {} → exit={}", ts_str, pid, desc, exit));
                    }
                }

                // Skip stats + log-lines blob by jumping to the next validated record.
                // We try a greedy parse of stats[5×7bit] + logCount[7bit] first;
                // fall back to the high-confidence scan if anything smells wrong.
                let try_pos = r.pos;
                let mut parse_ok = true;
                for _ in 0..5 { if r.read_7bit().is_none() { parse_ok = false; break; } }
                let n = if parse_ok { r.read_7bit().unwrap_or(u64::MAX) } else { u64::MAX };
                if !parse_ok || n > 4096 {
                    r.pos = find_validated_record(&data, try_pos);
                } else {
                    for _ in 0..n {
                        let ok = r.read_u8().is_some() && r.read_string().is_some();
                        if !ok { r.pos = find_validated_record(&data, try_pos); break; }
                    }
                }
            }

            T_PROGRESS_UPDATE => {
                let Some(total)  = r.read_7bit() else { r.pos = rec_start + 1; continue };
                let Some(done)   = r.read_7bit() else { r.pos = rec_start + 1; continue };
                let Some(errors) = r.read_7bit() else { r.pos = rec_start + 1; continue };
                // Sanity: skip garbage with astronomical counts or retrograde timestamps
                if !suspicious && total > 0 && total <= 100_000 && errors <= 10_000 {
                    let pct   = done * 100 / total;
                    let width = 24usize;
                    let filled = ((done * width as u64) / total) as usize;
                    let bar: String = "█".repeat(filled) + &"░".repeat(width - filled);
                    if errors > 0 {
                        out.push(format!("[{}] PROGRESS [{}] {}/{} {}%  errors={}", ts_str, bar, done, total, pct, errors));
                    } else {
                        out.push(format!("[{}] PROGRESS [{}] {}/{} {}%", ts_str, bar, done, total, pct));
                    }
                }
            }

            T_STATUS_UPDATE => {
                // statusRow(u32) + statusCol(u32) + text(string) + entryType(u8) + link(string)
                let Some(_row)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(_col)  = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(text)  = r.read_string() else { r.pos = rec_start + 1; continue };
                let Some(etype) = r.read_u8()     else { r.pos = rec_start + 1; continue };
                let Some(_link) = r.read_string() else { r.pos = rec_start + 1; continue };
                if !suspicious && !text.is_empty() && text.is_ascii() {
                    let lvl = match etype { 0 => "ERROR  ", 1 => "WARN   ", _ => "STATUS " };
                    out.push(format!("[{}] {}  {}", ts_str, lvl, text));
                }
            }

            // ── Skip-friendly records (fixed or short structure) ─────────────
            T_PROCESS_ENV_UPDATED => {
                // processId(u32) + envString(string)
                r.read_u32_le(); r.read_string();
            }
            T_REMOTE_EXEC_DISABLED => {
                // processId(u32) + reason(string)
                r.read_u32_le(); r.read_string();
            }
            T_SCHEDULER_KILL => {
                // processId(u32)
                r.read_u32_le();
            }

            // File / work / cache records: skip gracefully
            T_FILE_FETCH_BEGIN | T_FILE_FETCH_END |
            T_FILE_STORE_BEGIN | T_FILE_STORE_END |
            T_FILE_FETCH_LIGHT | T_FILE_STORE_LIGHT |
            T_FILE_FETCH_SIZE => {
                // workId(u32) + fileId(u32) or similar — try minimal read
                r.read_u32_le();
            }

            T_WORK_BEGIN | T_WORK_HINT | T_WORK_END => {
                // workId(u32) + optional data
                r.read_u32_le();
            }

            T_TASK_BEGIN | T_TASK_HINT | T_TASK_END => {
                r.read_u32_le();
            }

            T_CACHE_BEGIN_FETCH | T_CACHE_END_FETCH |
            T_CACHE_BEGIN_WRITE | T_CACHE_END_WRITE => {
                r.read_u32_le();
            }

            T_PROXY_CREATED | T_PROXY_USED => {
                r.read_u32_le();
            }

            T_DRIVE_UPDATE => {
                // driveId(u32) + available(u64) 7-bit + total(u64) 7-bit
                r.read_u32_le(); r.read_7bit(); r.read_7bit();
            }

            T_STRING_RECORD => {
                // stringId(u32) + text(string)
                r.read_u32_le(); r.read_string();
            }

            T_CACHE_SUMMARY => {
                // sessionId(u32) + hits(7bit) + misses(7bit) + stores(7bit)
                r.read_u32_le(); r.read_7bit(); r.read_7bit(); r.read_7bit();
            }

            T_SUMMARY => {
                // sessionId(u32) + N × (name:string + value:string)
                let Some(_sid) = r.read_u32_le() else { r.pos = rec_start + 1; continue };
                let Some(n)    = r.read_7bit()   else { r.pos = rec_start + 1; continue };
                let mut ok = true;
                for _ in 0..n.min(128) {
                    if r.read_string().is_none() || r.read_string().is_none() { ok = false; break; }
                }
                if !ok { r.pos = rec_start + 1; }
            }

            T_SCHEDULER_UPDATE => {
                // Complex scheduler state — just skip (scan to next record)
                r.pos = find_next_record(&data, rec_start + 1);
                consec_fail += 1;
            }

            T_SESSION_UPDATE => {
                // sessionId(u32) + connectionCount(7bit) + send(7bit) + recv(7bit)
                // + lastPing(7bit) + memAvail(7bit) + memTotal(7bit) + cpuLoad(f32)
                // cpuLoad is a raw f32 (4 bytes), everything else is 7-bit
                let ok = r.read_u32_le().is_some()  // sessionId
                    && r.read_7bit().is_some()       // connectionCount
                    && r.read_7bit().is_some()       // send
                    && r.read_7bit().is_some()       // recv
                    && r.read_7bit().is_some()       // lastPing
                    && r.read_7bit().is_some()       // memAvail
                    && r.read_7bit().is_some()       // memTotal
                    && r.skip(4);                    // cpuLoad (f32)
                if !ok { r.pos = find_next_record(&data, rec_start + 1); }
            }

            _ => {
                // Unknown type — scan forward to next plausible record start
                r.pos = find_next_record(&data, rec_start + 1);
                consec_fail += 1;
            }
        }
    }

    if out.len() <= 4 {
        out.push("  (No parseable records found — the UBA format may differ from TraceVersion 49)".to_string());
    }
    out.push(String::new());

    Ok(UbaParseResult { version, lines: out })
}

/// Scan forward from `from` until we find a byte in [0, T_MAX_VALID] that is
/// followed by a valid 7-bit sequence (≤5 bytes, reasonable timestamp value).
fn find_next_record(data: &[u8], from: usize) -> usize {
    let limit = data.len().min(from + 512);
    for i in from..limit {
        let b = data[i];
        if b > T_MAX_VALID { continue; }
        // Try reading a 7-bit timestamp starting at i+1
        let mut pos = i + 1;
        let mut ts: u64 = 0;
        let mut shift = 0u32;
        let mut valid = false;
        while pos < data.len() && shift <= 35 {
            let tb = data[pos];
            pos += 1;
            ts |= ((tb & 0x7F) as u64) << shift;
            shift += 7;
            if tb & 0x80 == 0 { valid = true; break; }
        }
        // Sanity: timestamp should be plausible (< ~10 trillion ticks = ~277 hours @ 10 MHz)
        if valid && ts < 10_000_000_000_000 {
            return i;
        }
    }
    from + 1
}

/// High-confidence forward scan: finds a recognisable record whose fields
/// validate semantically.  Used to skip over binary blobs of unknown size.
fn find_validated_record(data: &[u8], from: usize) -> usize {
    let limit = data.len().min(from + 16384);
    for i in from..limit {
        let type_byte = data[i];
        if type_byte > T_MAX_VALID { continue; }
        let mut r = Reader::new(data);
        r.pos = i + 1;
        let _ts = match r.read_7bit() { Some(v) => v, None => continue };
        let ok = match type_byte {
            T_PROGRESS_UPDATE => {
                let total = r.read_7bit().unwrap_or(u64::MAX);
                let done  = r.read_7bit().unwrap_or(u64::MAX);
                let errs  = r.read_7bit().unwrap_or(u64::MAX);
                total < 100_000 && done <= total && errs < 100_000
            }
            T_PROCESS_ADDED => {
                let sid  = r.read_u32_le().unwrap_or(u32::MAX);
                let pid  = r.read_u32_le().unwrap_or(u32::MAX);
                let desc = r.read_string();
                sid < 1_000 && pid < 100_000_000
                    && desc.as_deref().map(|s| s.len() >= 4 && s.is_ascii()).unwrap_or(false)
            }
            T_SESSION_INFO => {
                let sid  = r.read_u32_le().unwrap_or(u32::MAX);
                let text = r.read_string();
                sid < 1_000
                    && text.as_deref().map(|s| s.len() >= 4 && s.is_ascii()).unwrap_or(false)
            }
            T_SESSION_SUMMARY => {
                let sid = r.read_u32_le().unwrap_or(u32::MAX);
                let n   = r.read_7bit().unwrap_or(u64::MAX);
                sid < 1_000 && n < 1_000
            }
            T_STATUS_UPDATE => {
                let row  = r.read_u32_le().unwrap_or(u32::MAX);
                let col  = r.read_u32_le().unwrap_or(u32::MAX);
                let text = r.read_string();
                row < 65536 && col < 65536
                    && text.as_deref().map(|s| s.len() >= 4 && s.is_ascii()).unwrap_or(false)
            }
            _ => false,
        };
        if ok { return i; }
    }
    from + 1
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_7bit_decode() {
        // 0xF1 0x24: F1 has bit7=1 (cont), low7=0x71=113; 24 has bit7=0, low7=0x24=36
        // value = 113 | (36 << 7) = 113 + 4608 = 4721
        let data = [0xF1u8, 0x24];
        let mut r = Reader::new(&data);
        assert_eq!(r.read_7bit(), Some(4721));
    }

    #[test]
    fn test_7bit_single_byte() {
        let data = [0x26u8]; // 38, no continuation
        let mut r = Reader::new(&data);
        assert_eq!(r.read_7bit(), Some(38));
    }

    #[test]
    fn test_string_decode() {
        // length=3, then "abc"
        let data = [0x03u8, b'a', b'b', b'c'];
        let mut r = Reader::new(&data);
        assert_eq!(r.read_string(), Some("abc".to_string()));
    }

    #[test]
    fn test_find_next_record_skips_high_bytes() {
        // High bytes (> 38) should be skipped; 0x02 is ProcessAdded
        let data: Vec<u8> = vec![0xFF, 0xFF, 0x80, 0x02, 0x10]; // 0x02 at pos 3, ts=0x10=16
        let next = find_next_record(&data, 0);
        assert_eq!(next, 3);
    }
}
