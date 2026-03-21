use std::io::{Read, Seek, SeekFrom, BufReader};
use std::fs::File;
use std::path::Path;
use byteorder::{LittleEndian, ReadBytesExt};

pub struct UObjectImport {
    pub object_name: String,
    pub class_name: String,
    pub outer_index: i32,
}

pub struct UObjectExport {
    pub class_index: i32,
    pub super_index: i32,
    pub template_index: i32,
    pub outer_index: i32,
    pub object_name: String,
    pub serial_size: i64,
    pub serial_offset: i64,
}

pub struct UAssetParser {
    pub name_map: Vec<String>,
    pub import_map: Vec<UObjectImport>,
    pub export_map: Vec<UObjectExport>,
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
            export_map: Vec::new(),
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
            if length > 32768 { return Err(anyhow::anyhow!("String too long: {}", length)); }
            let mut buf = vec![0u8; length as usize];
            reader.read_exact(&mut buf)?;
            let s = String::from_utf8_lossy(&buf[..buf.len() - 1]).to_string();
            Ok(s)
        } else {
            let u16_len = -length;
            if u16_len > 16384 { return Err(anyhow::anyhow!("String too long: {}", u16_len)); }
            let mut buf = vec![0u16; u16_len as usize];
            for i in 0..u16_len { buf[i as usize] = reader.read_u16::<LittleEndian>()?; }
            let s = String::from_utf16_lossy(&buf[..buf.len() - 1]);
            Ok(s)
        }
    }

    fn resolve_path(&self, index: i32) -> String {
        self.resolve_path_recursive(index, 0)
    }

    fn resolve_path_recursive(&self, index: i32, depth: i32) -> String {
        if index == 0 || depth > 16 { return "None".to_string(); }
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
        } else if index > 0 {
            let idx = (index - 1) as usize;
            if let Some(exp) = self.export_map.get(idx) {
                if exp.outer_index != 0 {
                    let outer = self.resolve_path_recursive(exp.outer_index, depth + 1);
                    return format!("{}.{}", outer, exp.object_name);
                }
                return exp.object_name.clone();
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
        let ue4_ver = reader.read_i32::<LittleEndian>()?;
        let ue5_ver = if legacy_ver <= -8 { reader.read_i32::<LittleEndian>()? } else { 0 };
        let _licensee_ver = reader.read_i32::<LittleEndian>()?;

        if legacy_ver <= -9 { // UE5.6+
            let _ = reader.seek(SeekFrom::Current(20)); // SavedHash
            let _ = reader.read_i32::<LittleEndian>(); // TotalHeaderSize
        }

        if legacy_ver <= -2 {
            let count = reader.read_i32::<LittleEndian>()?;
            if count > 0 && count < 2000 {
                let _ = reader.seek(SeekFrom::Current(count as i64 * 20)); // CustomVersion
            }
        }

        if legacy_ver > -9 { let _ = reader.read_i32::<LittleEndian>(); } // TotalHeaderSize for pre-5.6
        
        let package_full_path = match Self::read_string(&mut reader) {
            Ok(s) => s,
            Err(_) => return Err(anyhow::anyhow!("Failed to read PackageName")),
        };
        self.asset_name = package_full_path.split('/').last().unwrap_or("").to_string();

        let package_flags = reader.read_u32::<LittleEndian>()?;
        let name_count = reader.read_i32::<LittleEndian>()?;
        let name_offset = reader.read_i32::<LittleEndian>()?;

        if ue5_ver >= 4 { let _ = reader.seek(SeekFrom::Current(8)); } // SoftObjectPaths
        if (package_flags & 0x80000000) == 0 { let _ = Self::read_string(&mut reader); } // LocalizationId
        let _ = reader.seek(SeekFrom::Current(8)); // GatherableTextData

        let export_count = reader.read_i32::<LittleEndian>()?;
        let export_offset = reader.read_i32::<LittleEndian>()?;
        let import_count = reader.read_i32::<LittleEndian>()?;
        let import_offset = reader.read_i32::<LittleEndian>()?;

        // SKIP additional Summary fields to reach the next part correctly
        let _ = reader.read_i32::<LittleEndian>(); // DependsOffset
        if ue4_ver >= 515 { let _ = reader.seek(SeekFrom::Current(8)); } // SoftPackageReferences
        if ue4_ver >= 516 { let _ = reader.read_i32::<LittleEndian>(); } // SearchableNamesOffset
        let _ = reader.read_i32::<LittleEndian>(); // ThumbnailTableOffset

        // UE 5.0 - 5.5: Guid (16 bytes)
        if legacy_ver > -9 {
            let _ = reader.seek(SeekFrom::Current(16));
        }

        if name_offset <= 0 || (name_offset as u64) >= file_size {
             return Err(anyhow::anyhow!("Invalid name map offset"));
        }

        // --- 2. Parse Name Map ---
        let _ = reader.seek(SeekFrom::Start(name_offset as u64));
        for _ in 0..name_count {
            if let Ok(sn) = Self::read_string(&mut reader) {
                self.name_map.push(sn);
                if ue4_ver >= 504 { let _ = reader.read_u32::<LittleEndian>(); } // Name Hash
            } else { break; }
        }

        // --- 3. Parse Import Map ---
        if import_offset > 0 && (import_offset as u64) < file_size {
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
        }

        // --- 4. Resolve imports ---
        for i in 0..self.import_map.len() {
            let path = self.resolve_path(-(i as i32) - 1);
            if path.starts_with('/') {
                if self.import_map[i].class_name == "Function" { self.functions.push(path); }
                else { self.imports.push(path); }
            }
        }

        // --- 5. Parse Export Map ---
        if export_count > 0 && export_offset > 0 && (export_offset as u64) < file_size {
            let _ = reader.seek(SeekFrom::Start(export_offset as u64));
            for _ in 0..export_count {
                let class_index = reader.read_i32::<LittleEndian>()?;
                let super_index = reader.read_i32::<LittleEndian>()?;
                let template_index = if ue4_ver >= 517 { reader.read_i32::<LittleEndian>()? } else { 0 };
                let outer_index = reader.read_i32::<LittleEndian>()?;
                let object_name_idx = reader.read_i64::<LittleEndian>()?;
                let _obj_flags = reader.read_u32::<LittleEndian>()?;
                
                let (serial_size, serial_offset) = if ue4_ver >= 511 {
                    (reader.read_i64::<LittleEndian>()?, reader.read_i64::<LittleEndian>()?)
                } else {
                    (reader.read_i32::<LittleEndian>()? as i64, reader.read_i32::<LittleEndian>()? as i64)
                };

                // Skip remainder of export entry based on version
                let _forced_export = reader.read_i32::<LittleEndian>()?;
                let _not_for_client = reader.read_i32::<LittleEndian>()?;
                let _not_for_server = reader.read_i32::<LittleEndian>()?;
                
                if ue5_ver < 1 { let _ = reader.seek(SeekFrom::Current(16)); } // Package Guid
                if ue5_ver >= 6 { let _ = reader.read_i32::<LittleEndian>()?; } // IsInheritedInstance
                let _ = reader.read_u32::<LittleEndian>()?; // PackageFlags
                if ue4_ver >= 384 { let _ = reader.read_i32::<LittleEndian>()?; } // NotAlwaysLoadedForEditorGame
                if ue4_ver >= 510 { let _ = reader.read_i32::<LittleEndian>()?; } // IsAsset
                if ue5_ver >= 7 { let _ = reader.read_i32::<LittleEndian>()?; } // GeneratePublicHash
                
                if ue4_ver >= 504 { // Preload Dependencies
                    let _ = reader.seek(SeekFrom::Current(20));
                }
                
                if ue5_ver >= 1 { // ScriptSerializationOffsets
                    let _ = reader.seek(SeekFrom::Current(16));
                }

                let object_name = self.name_map.get(object_name_idx as usize).cloned().unwrap_or_default();
                self.export_map.push(UObjectExport {
                    class_index,
                    super_index,
                    template_index,
                    outer_index,
                    object_name,
                    serial_size,
                    serial_offset,
                });
            }

            // --- 6. Resolve Parent from Export Map ---
            let mut bp_parent = None;
            for exp in &self.export_map {
                if exp.object_name == self.asset_name || exp.object_name == format!("{}_C", self.asset_name) {
                    if exp.super_index != 0 {
                        bp_parent = Some(self.resolve_path(exp.super_index));
                        break;
                    }
                }
                if bp_parent.is_none() && exp.outer_index == 0 && exp.class_index < 0 {
                    let path = self.resolve_path(exp.class_index);
                    if path.starts_with("/Script/") && !path.contains("BlueprintGeneratedClass") {
                        bp_parent = Some(path);
                    }
                }
            }
            self.parent_class = bp_parent;
        }

        Ok(())
    }
}
