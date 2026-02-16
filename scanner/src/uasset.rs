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
    pub functions: Vec<String>,
    pub parent_class: Option<String>,
    pub asset_name: String,
}

impl UAssetParser {
    pub fn new() -> Self {
        Self {
            name_map: Vec::new(),
            import_map: Vec::new(),
            imports: Vec::new(),
            functions: Vec::new(),
            parent_class: None,
            asset_name: String::new(),
        }
    }

    fn read_string<R: Read + Seek>(reader: &mut R) -> anyhow::Result<String> {
        let length = reader.read_i32::<LittleEndian>()?;
        if length == 0 { return Ok(String::new()); }
        if length > 0 {
            if length > 2048 { return Err(anyhow::anyhow!("String too long")); }
            let mut buf = vec![0u8; length as usize];
            reader.read_exact(&mut buf)?;
            if buf.is_empty() { return Ok(String::new()); }
            let s = String::from_utf8_lossy(&buf[..buf.len() - 1]).to_string();
            Ok(s)
        } else {
            let u16_len = -length;
            if u16_len > 2048 { return Err(anyhow::anyhow!("String too long")); }
            let mut buf = vec![0u16; u16_len as usize];
            for i in 0..u16_len { buf[i as usize] = reader.read_u16::<LittleEndian>()?; }
            if buf.len() < 1 { return Ok(String::new()); }
            let s = String::from_utf16_lossy(&buf[..buf.len() - 1]);
            Ok(s)
        }
    }

    fn resolve_path(&self, index: i32) -> String {
        self.resolve_path_recursive(index, 0)
    }

    fn resolve_path_recursive(&self, index: i32, depth: i32) -> String {
        if index == 0 || depth > 10 { return "None".to_string(); }
        if index < 0 {
            let idx = (-index - 1) as usize;
            if let Some(imp) = self.import_map.get(idx) {
                let mut obj_name = imp.object_name.clone();
                if obj_name.starts_with("Default__") { obj_name = obj_name.replace("Default__", ""); }
                
                if imp.outer_index != 0 {
                    let outer = self.resolve_path_recursive(imp.outer_index, depth + 1);
                    if outer.starts_with('/') {
                        let separator = if imp.class_name == "Function" { ":" } else { "." };
                        return format!("{}{}{}", outer, separator, obj_name);
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
        let file_size = file.metadata()?.len();
        let mut reader = BufReader::new(&mut file);

        let tag = reader.read_u32::<LittleEndian>()?;
        if tag != 0x9E2A83C1 && tag != 0xC1832A9E { return Err(anyhow::anyhow!("Invalid tag")); }

        let legacy_ver = reader.read_i32::<LittleEndian>()?;
        if legacy_ver != -4 { let _ = reader.read_i32::<LittleEndian>(); }
        let _ue4_ver = reader.read_i32::<LittleEndian>()?;
        let ue5_ver = if legacy_ver <= -8 { reader.read_i32::<LittleEndian>()? } else { 0 };
        let _licensee_ver = reader.read_i32::<LittleEndian>()?;

        if ue5_ver >= 7 {
            let _ = reader.seek(SeekFrom::Current(20)); // SavedHash
            let _ = reader.read_i32::<LittleEndian>(); // TotalHeaderSize
        }

        if legacy_ver <= -2 {
            let count = reader.read_i32::<LittleEndian>()?;
            if count > 0 && count < 2000 {
                let _ = reader.seek(SeekFrom::Current(count as i64 * 20));
            }
        }

        if ue5_ver < 7 { let _ = reader.read_i32::<LittleEndian>(); }
        
        let package_full_path = match Self::read_string(&mut reader) {
            Ok(s) => s,
            Err(_) => return Err(anyhow::anyhow!("Failed to read PackageName")),
        };
        self.asset_name = package_full_path.split('/').last().unwrap_or("").to_string();

        let package_flags = reader.read_u32::<LittleEndian>()?;
        let name_count = reader.read_i32::<LittleEndian>()?;
        let name_offset = reader.read_i32::<LittleEndian>()?;

        if ue5_ver >= 4 { let _ = reader.seek(SeekFrom::Current(8)); }
        if (package_flags & 0x80000000) == 0 { let _ = Self::read_string(&mut reader); }
        let _ = reader.seek(SeekFrom::Current(8)); // GatherableTextData

        let export_count = reader.read_i32::<LittleEndian>()?;
        let export_offset = reader.read_i32::<LittleEndian>()?;
        let import_count = reader.read_i32::<LittleEndian>()?;
        let import_offset = reader.read_i32::<LittleEndian>()?;

        if name_offset <= 0 || import_offset <= 0 || (import_offset as u64) >= file_size {
             return Err(anyhow::anyhow!("Invalid map offsets"));
        }

        // --- 2. Parse Name Map ---
        let _ = reader.seek(SeekFrom::Start(name_offset as u64));
        for _ in 0..name_count {
            if let Ok(sn) = Self::read_string(&mut reader) {
                self.name_map.push(sn);
                let _ = reader.read_u32::<LittleEndian>();
            } else { break; }
        }

        // --- 3. Parse Import Map ---
        let _ = reader.seek(SeekFrom::Start(import_offset as u64));
        for _ in 0..import_count {
            let _class_package = reader.read_i64::<LittleEndian>()?;
            let class_name_idx = reader.read_i64::<LittleEndian>()? as i32;
            let outer_index = reader.read_i32::<LittleEndian>()?;
            let object_name_idx = reader.read_i64::<LittleEndian>()? as i32;
            
            if (package_flags & 0x80000000) == 0 { let _ = reader.read_i64::<LittleEndian>(); }
            if ue5_ver >= 12 { let _ = reader.read_u32::<LittleEndian>(); }

            let obj_name = self.name_map.get(object_name_idx as usize).cloned().unwrap_or_default();
            let cls_name = self.name_map.get(class_name_idx as usize).cloned().unwrap_or_default();
            self.import_map.push(UObjectImport { object_name: obj_name, class_name: cls_name, outer_index: outer_index });
        }

        // --- 4. Resolve imports ---
        for i in 0..self.import_map.len() {
            let path = self.resolve_path(-(i as i32) - 1);
            if path.starts_with('/') {
                if self.import_map[i].class_name == "Function" { self.functions.push(path); }
                else { self.imports.push(path); }
            }
        }

        // --- 5. Resolve Parent Class from Export Map ---
        if export_count > 0 && export_offset > 0 {
            let _ = reader.seek(SeekFrom::Start(export_offset as u64));
            let mut bp_parent = None;
            let mut fallback_parent = None;
            
            for _ in 0..export_count {
                let entry_start = reader.stream_position().unwrap();
                let class_idx = reader.read_i32::<LittleEndian>()?;
                let super_idx = reader.read_i32::<LittleEndian>()?;
                let _tmp = reader.read_i32::<LittleEndian>()?;
                let outer_idx = reader.read_i32::<LittleEndian>()?;
                let object_name_idx = reader.read_i64::<LittleEndian>()?;
                
                // Identify the main class of the asset (Blueprint or DataAsset)
                if class_idx < 0 {
                    let _object_name = self.name_map.get(object_name_idx as usize).cloned().unwrap_or_default();
                    let _class_type = if class_idx < 0 {
                        self.import_map.get((-class_idx - 1) as usize).map(|i| i.object_name.clone()).unwrap_or_default()
                    } else { String::new() };

                    // LOGIC: A Blueprint's main class is often named 'BlueprintGeneratedClass'.
                    // If we find it, its SuperIndex is the definitive C++ parent.
                    if _class_type.contains("GeneratedClass") {
                        if super_idx != 0 {
                            bp_parent = Some(self.resolve_path(super_idx));
                            break; 
                        }
                    }
                    
                    // Fallback: If it's a root object (Outer == 0) and has a valid class, track it.
                    if fallback_parent.is_none() && outer_idx == 0 && class_idx < 0 {
                        let path = self.resolve_path(class_idx);
                        if path.starts_with('/') && !path.contains("BlueprintGeneratedClass") {
                            fallback_parent = Some(path);
                        }
                    }
                }

                // Correctly skip the rest of this FObjectExport entry
                let mut skip = if ue5_ver >= 7 { 16 } else { 8 };
                skip += 12;
                if ue5_ver < 7 { skip += 16; }
                skip += 12;
                if ue5_ver >= 1 { skip += 4; }
                if ue5_ver >= 7 { skip += 36; }
                let _ = reader.seek(SeekFrom::Start(entry_start + 32 + skip));
            }
            self.parent_class = bp_parent.or(fallback_parent);
        }

        Ok(())
    }
}
