/// EXN format: Kinetix Executable Bundle (.exki)
///
/// Layout:
/// [4 bytes] Magic: "KNTX"
/// [4 bytes] Manifest length (u32 LE)
/// [N bytes] JSON manifest
/// [remaining] JSON-serialized CompiledProgram

use crate::ir::CompiledProgram;
use std::io::{self, Read, Write};

const MAGIC: &[u8; 4] = b"KNTX";

/// Serialize a CompiledProgram to the .exki binary format.
pub fn write_exn<W: Write>(writer: &mut W, program: &CompiledProgram) -> io::Result<()> {
    // 1. Magic number
    writer.write_all(MAGIC)?;

    // 2. JSON manifest
    let manifest = serde_json::json!({
        "version": program.version,
        "functions": program.functions.len(),
        "format": "kivm-bytecode-v1",
    });
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let manifest_len = manifest_bytes.len() as u32;
    writer.write_all(&manifest_len.to_le_bytes())?;
    writer.write_all(&manifest_bytes)?;

    // 3. Bytecode: serialize the entire program as JSON (simple, portable)
    let bytecode = serde_json::to_vec(program)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let bytecode_len = bytecode.len() as u32;
    writer.write_all(&bytecode_len.to_le_bytes())?;
    writer.write_all(&bytecode)?;

    Ok(())
}

/// Deserialize a CompiledProgram from .exki binary format.
pub fn read_exn<R: Read>(reader: &mut R) -> io::Result<CompiledProgram> {
    // 1. Validate magic
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid magic number: expected KNTX, got {:?}", magic),
        ));
    }

    // 2. Read manifest
    let mut manifest_len_bytes = [0u8; 4];
    reader.read_exact(&mut manifest_len_bytes)?;
    let manifest_len = u32::from_le_bytes(manifest_len_bytes) as usize;
    let mut manifest_bytes = vec![0u8; manifest_len];
    reader.read_exact(&mut manifest_bytes)?;
    // Manifest is informational; we don't strictly need it to run.

    // 3. Read bytecode
    let mut bytecode_len_bytes = [0u8; 4];
    reader.read_exact(&mut bytecode_len_bytes)?;
    let bytecode_len = u32::from_le_bytes(bytecode_len_bytes) as usize;
    let mut bytecode_bytes = vec![0u8; bytecode_len];
    reader.read_exact(&mut bytecode_bytes)?;

    let program: CompiledProgram = serde_json::from_slice(&bytecode_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    #[test]
    fn test_exn_roundtrip() {
        let mut program = CompiledProgram::new();
        program.main.emit(Instruction::a_only(Opcode::LoadNull, 0));
        program.main.emit(Instruction::a_only(Opcode::Halt, 0));
        program.main.add_constant(Constant::Integer(42));
        program.main.add_constant(Constant::String("test".to_string()));

        let mut buf: Vec<u8> = Vec::new();
        write_exn(&mut buf, &program).expect("write failed");

        let mut cursor = std::io::Cursor::new(buf);
        let loaded = read_exn(&mut cursor).expect("read failed");

        assert_eq!(loaded.main.instructions.len(), 2);
        assert_eq!(loaded.main.constants.len(), 2);
        assert_eq!(loaded.main.constants[0], Constant::Integer(42));
        assert_eq!(loaded.version, "0.1.0");
    }

    #[test]
    fn test_exn_invalid_magic() {
        let buf = b"BAAD\x00\x00\x00\x00";
        let mut cursor = std::io::Cursor::new(buf.to_vec());
        let result = read_exn(&mut cursor);
        assert!(result.is_err());
    }
}
