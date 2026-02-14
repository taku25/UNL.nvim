use std::io::{Read, Seek, SeekFrom, BufReader};
use std::fs::File;
use std::path::Path;
use byteorder::{LittleEndian, ReadBytesExt};

pub struct UObjectImport {
    pub object_name: String,
    pub class_name: String,
    pub outer_index: i32,
}

pub struct UAssetParser {
    pub name_map: Vec<String>,
    pub import_map: Vec<UObjectImport>,
    pub imports: Vec<String>,
    pub parent_class: Option<String>,
    pub asset_name: String,
}

impl UAssetParser {
    pub fn new() -> Self {
        Self {
            name_map: Vec::new(),
            import_map: Vec::new(),
            imports: Vec::new(),
            parent_class: None,
            asset_name: String::new(),
        }
    }

    fn read_string<R: Read + Seek>(reader: &mut R) -> anyhow::Result<String> {
        let length = reader.read_i32::<LittleEndian>()?;
        if length == 0 { return Ok(String::new()); }
        if length > 0 {
            if length > 4096 { return Err(anyhow::anyhow!("String too long")); }
            let mut buf = vec![0u8; length as usize];
            reader.read_exact(&mut buf)?;
            let s = String::from_utf8_lossy(&buf[..buf.len() - 1]).to_string();
            Ok(s)
        } else {
            let u16_len = -length;
            if u16_len > 4096 { return Err(anyhow::anyhow!("String too long")); }
            let mut buf = vec![0u16; u16_len as usize];
            for i in 0..u16_len { buf[i as usize] = reader.read_u16::<LittleEndian>()?; }
            let s = String::from_utf16_lossy(&buf[..buf.len() - 1]);
            Ok(s)
        }
    }

    fn resolve_path(&self, index: i32) -> String {
        if index == 0 { return "None".to_string(); }
        if index < 0 {
            let idx = (-index - 1) as usize;
            if let Some(imp) = self.import_map.get(idx) {
                let mut obj_name = imp.object_name.clone();
                if obj_name.starts_with("Default__") { obj_name = obj_name.replace("Default__", ""); }
                if imp.outer_index != 0 {
                    let outer = self.resolve_path(imp.outer_index);
                    if outer.starts_with('/') {
                        if outer.contains('.') { return outer; }
                        return format!("{}.{}", outer, obj_name);
                    }
                    return format!("{}/{}", outer, obj_name);
                }
                return obj_name;
            }
        }
        "None".to_string()
    }

    pub fn parse<P: AsRef<Path>>(&mut self, path: P) -> anyhow::Result<()> {
        let mut file = File::open(path)?;
        let mut reader = BufReader::new(&mut file);

        let tag = reader.read_u32::<LittleEndian>()?;
        if tag != 0x9E2A83C1 { return Err(anyhow::anyhow!("Invalid tag")); }

        let legacy_version = reader.read_i32::<LittleEndian>()?;
        let _ue3 = reader.read_i32::<LittleEndian>()?;
        let ver_ue4 = reader.read_i32::<LittleEndian>()?;
        let ver_ue5 = if legacy_version <= -8 { reader.read_i32::<LittleEndian>()? } else { 0 };
        
        // --- 1. Find Summary Header by searching for PackageName string ---
        let mut found_summary = false;
        let mut package_full_path = String::new();
        // Scan a wide range (0x40 to 0x400) because UE5 headers can be large
        for search_pos in (0x40..0x400).step_by(1) {
            let _ = reader.seek(SeekFrom::Start(search_pos));
            if let Ok(s_len) = reader.read_i32::<LittleEndian>() {
                if s_len > 10 && s_len < 512 {
                    let mut buf = vec![0u8; s_len as usize];
                    if reader.read_exact(&mut buf).is_ok() {
                        if buf[0] == b'/' {
                            let name = String::from_utf8_lossy(&buf[..buf.len()-1]);
                            if name.starts_with("/Game/") || name.starts_with("/Engine/") || name.starts_with("/Plugin/") {
                                package_full_path = name.to_string();
                                let _ = reader.seek(SeekFrom::Start(search_pos - 4));
                                found_summary = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if !found_summary { return Err(anyhow::anyhow!("Summary header not found")); }

        self.asset_name = package_full_path.split('/').last().unwrap_or("").to_string();

        let _ths = reader.read_i32::<LittleEndian>()?;
        let _pn = Self::read_string(&mut reader)?;
        let package_flags = reader.read_u32::<LittleEndian>()?;
        let name_count = reader.read_i32::<LittleEndian>()?;
        let name_offset = reader.read_i32::<LittleEndian>()?;

        if ver_ue5 >= 1008 { let _ = reader.read_i32::<LittleEndian>(); let _ = reader.read_i32::<LittleEndian>(); }
        let has_editor_data = (package_flags & 0x80000000) == 0;
        if ver_ue4 >= 516 && has_editor_data { let _ = Self::read_string(&mut reader); }
        if ver_ue4 >= 459 { let _ = reader.read_i32::<LittleEndian>(); let _ = reader.read_i32::<LittleEndian>(); }

        let export_count = reader.read_i32::<LittleEndian>()?;
        let export_offset = reader.read_i32::<LittleEndian>()?;
        let import_count = reader.read_i32::<LittleEndian>()?;
        let import_offset = reader.read_i32::<LittleEndian>()?;

        // --- 2. Parse Name Map ---
        reader.seek(SeekFrom::Start(name_offset as u64))?;
        for _ in 0..name_count {
            if let Ok(s) = Self::read_string(&mut reader) {
                if ver_ue4 >= 504 { let _ = reader.read_u32::<LittleEndian>(); }
                self.name_map.push(s);
            }
        }

        // --- 3. Parse Import Map ---
        reader.seek(SeekFrom::Start(import_offset as u64))?;
        for _ in 0..import_count {
            let _cp = reader.read_i64::<LittleEndian>()?;
            let cn_idx = reader.read_i64::<LittleEndian>()? as i32;
            let outer = reader.read_i32::<LittleEndian>()?;
            let obj_idx = reader.read_i64::<LittleEndian>()? as i32;

            let obj_name = self.name_map.get(obj_idx as usize).cloned().unwrap_or_default();
            let cls_name = self.name_map.get(cn_idx as usize).cloned().unwrap_or_default();
            self.import_map.push(UObjectImport { object_name: obj_name, class_name: cls_name, outer_index: outer });

            if ver_ue4 >= 520 && has_editor_data { let _ = reader.read_i64::<LittleEndian>(); }
            if ver_ue5 >= 1003 { let _ = reader.read_u32::<LittleEndian>(); }
        }

        // --- 4. Resolve all imports to full paths ---
        for i in 0..self.import_map.len() {
            let path = self.resolve_path(-(i as i32) - 1);
            if path.starts_with('/') {
                self.imports.push(path);
            }
        }

        // --- 5. Parse Export Map (Resolve Parent) ---
        if export_count > 0 {
            reader.seek(SeekFrom::Start(export_offset as u64))?;
            for _ in 0..export_count {
                let class_idx = reader.read_i32::<LittleEndian>()?;
                let super_idx = reader.read_i32::<LittleEndian>()?;
                let _tmp = reader.read_i32::<LittleEndian>()?;
                let _out = reader.read_i32::<LittleEndian>()?;
                let _obj_name_idx = reader.read_i64::<LittleEndian>()?;
                let _obj_flags = reader.read_u32::<LittleEndian>()?;

                let class_type = if class_idx < 0 {
                    self.import_map.get((-class_idx - 1) as usize).map(|i| i.object_name.clone()).unwrap_or_default()
                } else { String::new() };

                // Correct identification of Blueprint main class
                if class_type.contains("GeneratedClass") || class_type == "LinkedAnimGraphLibrary" {
                    if super_idx != 0 {
                        let resolved = self.resolve_path(super_idx);
                        self.parent_class = Some(resolved);
                        break;
                    }
                }

                // Skip remainder of FObjectExport
                let mut skip = if ver_ue4 >= 511 { 16 } else { 8 }; // SerialSize, SerialOffset
                skip += 12; // bForcedExport, bNotForClient, bNotForServer
                if ver_ue5 < 1005 { skip += 16; } // Guid
                if ver_ue5 >= 1006 { skip += 4; } // bIsInheritedInstance
                skip += 4; // PackageFlags
                if ver_ue4 >= 365 { skip += 4; } // bNotAlwaysLoadedForEditorGame
                if ver_ue4 >= 485 { skip += 4; } // bIsAsset
                if ver_ue5 >= 1003 { skip += 4; } // bGeneratePublicHash
                if ver_ue4 >= 507 { skip += 20; } // Dependencies
                if ver_ue5 >= 1010 { skip += 16; } // ScriptSerialization
                let _ = reader.seek(SeekFrom::Current(skip));
            }
        }

        Ok(())
    }
}