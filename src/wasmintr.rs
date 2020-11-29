// WebAssembly Interpreter

use super::opcode::*;
use super::wasm::*;
use crate::*;
use alloc::vec::Vec;

#[allow(dead_code)]
pub struct WasmInterpreter {}

impl WasmInterpreter {
    /// Interpret WebAssembly code blocks
    pub fn run(
        mut code_block: &mut WasmCodeBlock,
        locals: &mut [WasmValue],
        result_types: &[WasmValType],
        module: &WasmModule,
    ) -> Result<WasmValue, WasmRuntimeError> {
        let mut locals = {
            let mut output = Vec::with_capacity(locals.len());
            for local in locals {
                output.push(WasmStackValue::from(*local));
            }
            output
        };
        let mut value_stack: Vec<WasmStackValue> =
            Vec::with_capacity(code_block.info().max_stack());
        let mut block_stack = Vec::with_capacity(code_block.info().max_block_level());

        code_block.reset();
        loop {
            let opcode = code_block.read_opcode()?;

            // println!("{:04x} {:02x} {}", position, opcode as u8, opcode.to_str());

            match opcode {
                WasmOpcode::Nop => (),

                WasmOpcode::Block => {
                    let _ = code_block.read_uint()?;
                    block_stack.push(code_block.fetch_position());
                }
                WasmOpcode::Loop => {
                    let _ = code_block.read_uint()?;
                    block_stack.push(code_block.fetch_position());
                }
                WasmOpcode::If => {
                    let _ = code_block.read_uint()?;
                    let position = code_block.fetch_position();
                    let cc = value_stack
                        .pop()
                        .map(|v| v.get_bool())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if cc {
                        block_stack.push(position);
                    } else {
                        let block = code_block
                            .info()
                            .block_info(position)
                            .ok_or(WasmRuntimeError::InternalInconsistency)?;
                        let end_position = block.end_position;
                        let else_position = block.else_position;
                        if else_position != 0 {
                            block_stack.push(position);
                            code_block.set_position(else_position);
                        } else {
                            code_block.set_position(end_position);
                        }
                    }
                }
                WasmOpcode::Else => {
                    Self::branch(0, &mut block_stack, &mut value_stack, &mut code_block)?;
                }
                WasmOpcode::End => {
                    if block_stack.pop().is_none() {
                        break;
                    }
                }
                WasmOpcode::Br => {
                    let target = code_block.read_uint()? as usize;
                    Self::branch(target, &mut block_stack, &mut value_stack, &mut code_block)?;
                }
                WasmOpcode::BrIf => {
                    let target = code_block.read_uint()? as usize;
                    let cc = value_stack
                        .pop()
                        .map(|v| v.get_bool())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if cc {
                        Self::branch(target, &mut block_stack, &mut value_stack, &mut code_block)?;
                    }
                }
                WasmOpcode::BrTable => {
                    let mut index = value_stack
                        .pop()
                        .map(|v| v.get_i32() as usize)
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let n_vec = code_block.read_uint()? as usize;
                    if index >= n_vec {
                        index = n_vec + 1;
                    }
                    for _ in 0..=index {
                        let _ = code_block.read_uint()?;
                    }
                    let target = code_block.read_uint()? as usize;
                    Self::branch(target, &mut block_stack, &mut value_stack, &mut code_block)?;
                }

                WasmOpcode::Return => {
                    break;
                }

                WasmOpcode::Call => {
                    let index = code_block.read_uint()? as usize;
                    let func = module
                        .functions()
                        .get(index)
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;

                    let body = func.body().unwrap();

                    let mut locals = Vec::new();
                    let param_len = body.param_types().len();
                    if value_stack.len() < param_len {
                        return Err(WasmRuntimeError::InternalInconsistency);
                    }
                    let new_stack_len = value_stack.len() - param_len;
                    let params = &value_stack[new_stack_len..];
                    for (index, val_type) in body.param_types().iter().enumerate() {
                        locals.push(params[index].get_by_type(*val_type));
                    }
                    value_stack.resize(new_stack_len, WasmStackValue::from_usize(0));
                    for local in body.local_types() {
                        locals.push(WasmValue::default_for(*local));
                    }

                    let result_types = body.result_types();
                    let cb = body.code_block();
                    let cb_ref = cb.borrow();
                    let slice = cb_ref.as_slice();
                    let mut code_block = WasmCodeBlock::from_slice(slice, body.block_info());
                    let result = Self::run(&mut code_block, &mut locals, result_types, module)?;
                    if !result.is_empty() {
                        value_stack.push(WasmStackValue::from(result));
                    }
                }

                WasmOpcode::Drop => {
                    let _ = value_stack.pop();
                }
                WasmOpcode::Select => {
                    let cc = value_stack
                        .pop()
                        .map(|v| v.get_bool())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let b = value_stack
                        .pop()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let c = if cc { a } else { b };
                    value_stack.push(c);
                }

                WasmOpcode::LocalGet => {
                    let local_ref = code_block.read_uint()? as usize;
                    let val = *locals
                        .get(local_ref)
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(val.into());
                }
                WasmOpcode::LocalSet => {
                    let local_ref = code_block.read_uint()? as usize;
                    let var = locals
                        .get_mut(local_ref)
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = value_stack
                        .pop()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *var = val;
                }
                WasmOpcode::LocalTee => {
                    let local_ref = code_block.read_uint()? as usize;
                    let var = locals
                        .get_mut(local_ref)
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = value_stack
                        .last()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *var = *val;
                }

                WasmOpcode::I32Load => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u32(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val))
                }
                WasmOpcode::I32Store => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u32(memarg.offset_by(offset), val)?;
                }
                WasmOpcode::I64Load => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u64(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val))
                }
                WasmOpcode::I64Store => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u64(memarg.offset_by(offset), val)?;
                }

                WasmOpcode::I32Load8S => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u8(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as i8 as i32))
                }
                WasmOpcode::I32Load8U => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u8(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as u32))
                }
                WasmOpcode::I32Load16S => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u16(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as i16 as i32))
                }
                WasmOpcode::I32Load16U => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u16(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as u32))
                }

                WasmOpcode::I32Store8 => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u8())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u8(memarg.offset_by(offset), val)?;
                }
                WasmOpcode::I32Store16 => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u16())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u16(memarg.offset_by(offset), val)?;
                }

                WasmOpcode::I64Load8S => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u8(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as i8 as i64))
                }
                WasmOpcode::I64Load8U => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u8(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as u64))
                }
                WasmOpcode::I64Load16S => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u16(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as i16 as i64))
                }
                WasmOpcode::I64Load16U => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u16(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as u64))
                }
                WasmOpcode::I64Load32S => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u32(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as i32 as i64))
                }
                WasmOpcode::I64Load32U => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let val = memory.read_u32(memarg.offset_by(offset))?;
                    value_stack.push(WasmStackValue::from(val as u64))
                }

                WasmOpcode::I64Store8 => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u8())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u8(memarg.offset_by(offset), val)?;
                }
                WasmOpcode::I64Store16 => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u16())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u16(memarg.offset_by(offset), val)?;
                }
                WasmOpcode::I64Store32 => {
                    let memarg = code_block.read_memarg()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    let val = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let offset = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    memory.write_u32(memarg.offset_by(offset), val)?;
                }

                WasmOpcode::MemorySize => {
                    let _ = code_block.read_uint()?;
                    let memory = module.memory(0).ok_or(WasmRuntimeError::OutOfMemory)?;
                    value_stack.push(WasmStackValue::from(memory.size()));
                }

                WasmOpcode::I32Const => {
                    let val = code_block.read_sint()? as i32;
                    value_stack.push(WasmStackValue { i32: val });
                }
                WasmOpcode::I64Const => {
                    let val = code_block.read_sint()?;
                    value_stack.push(WasmStackValue { i64: val });
                }

                WasmOpcode::I32Eqz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from(last.get_i32() == 0);
                }
                WasmOpcode::I32Eq => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a == b));
                }
                WasmOpcode::I32Ne => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a != b));
                }
                WasmOpcode::I32LtS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a < b));
                }
                WasmOpcode::I32LtU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a < b));
                }
                WasmOpcode::I32LeS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a <= b));
                }
                WasmOpcode::I32LeU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a <= b));
                }
                WasmOpcode::I32GtS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a > b));
                }
                WasmOpcode::I32GtU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a > b));
                }
                WasmOpcode::I32GeS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a >= b));
                }
                WasmOpcode::I32GeU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a >= b));
                }

                WasmOpcode::I64Eqz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from(last.get_i64() == 0);
                }
                WasmOpcode::I64Eq => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a == b));
                }
                WasmOpcode::I64Ne => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a != b));
                }
                WasmOpcode::I64LtS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a < b));
                }
                WasmOpcode::I64LtU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a < b));
                }
                WasmOpcode::I64LeS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a <= b));
                }
                WasmOpcode::I64LeU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a <= b));
                }
                WasmOpcode::I64GtS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a > b));
                }
                WasmOpcode::I64GtU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a > b));
                }
                WasmOpcode::I64GeS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a >= b));
                }
                WasmOpcode::I64GeU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from(a >= b));
                }

                WasmOpcode::I32Clz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_u32(last.get_i32().leading_zeros());
                }
                WasmOpcode::I32Ctz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_u32(last.get_i32().trailing_zeros());
                }
                WasmOpcode::I32Popcnt => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_u32(last.get_i32().count_ones());
                }

                WasmOpcode::I32Add => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a.wrapping_add(b)));
                }
                WasmOpcode::I32Sub => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a.wrapping_sub(b)));
                }
                WasmOpcode::I32Mul => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a.wrapping_mul(b)));
                }
                WasmOpcode::I32DivS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_i32(a.wrapping_div(b)));
                }
                WasmOpcode::I32DivU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_u32(a.wrapping_div(b)));
                }
                WasmOpcode::I32RemS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_i32(a.wrapping_rem(b)));
                }
                WasmOpcode::I32RemU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_u32(a.wrapping_rem(b)));
                }

                WasmOpcode::I32And => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a & b));
                }
                WasmOpcode::I32Or => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a | b));
                }
                WasmOpcode::I32Xor => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a ^ b));
                }

                WasmOpcode::I32Shl => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a << b));
                }
                WasmOpcode::I32ShrS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_i32(a >> b));
                }
                WasmOpcode::I32ShrU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a >> b));
                }
                WasmOpcode::I32Rotl => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a.rotate_left(b)));
                }
                WasmOpcode::I32Rotr => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u32())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u32(a.rotate_right(b)));
                }

                WasmOpcode::I64Clz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64(last.get_i64().leading_zeros() as i64);
                }
                WasmOpcode::I64Ctz => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64(last.get_i64().trailing_zeros() as i64);
                }
                WasmOpcode::I64Popcnt => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64(last.get_i64().count_ones() as i64);
                }

                WasmOpcode::I64Add => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a.wrapping_add(b)));
                }
                WasmOpcode::I64Sub => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a.wrapping_sub(b)));
                }
                WasmOpcode::I64Mul => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a.wrapping_mul(b)));
                }
                WasmOpcode::I64DivS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_i64(a.wrapping_div(b)));
                }
                WasmOpcode::I64DivU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_u64(a.wrapping_div(b)));
                }
                WasmOpcode::I64RemS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_i64(a.wrapping_rem(b)));
                }
                WasmOpcode::I64RemU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    if b == 0 {
                        return Err(WasmRuntimeError::DivideByZero);
                    }
                    value_stack.push(WasmStackValue::from_u64(a.wrapping_rem(b)));
                }

                WasmOpcode::I64And => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a & b));
                }
                WasmOpcode::I64Or => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a | b));
                }
                WasmOpcode::I64Xor => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a ^ b));
                }

                WasmOpcode::I64Shl => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a << b));
                }
                WasmOpcode::I64ShrS => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_i64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_i64(a >> b));
                }
                WasmOpcode::I64ShrU => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a >> b));
                }
                WasmOpcode::I64Rotl => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a.rotate_left(b as u32)));
                }
                WasmOpcode::I64Rotr => {
                    let b = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    let a = value_stack
                        .pop()
                        .map(|v| v.get_u64())
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    value_stack.push(WasmStackValue::from_u64(a.rotate_right(b as u32)));
                }

                WasmOpcode::I32WrapI64 => {
                    // NOP
                }
                WasmOpcode::I64ExtendI32S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64(last.get_i32() as i64);
                }
                WasmOpcode::I64ExtendI32U => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_u64(last.get_u32() as u64);
                }

                WasmOpcode::I32Extend8S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i32((last.get_i32() as i8) as i32);
                }
                WasmOpcode::I32Extend16S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i32((last.get_i32() as i16) as i32);
                }

                WasmOpcode::I64Extend8S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64((last.get_i64() as i8) as i64);
                }
                WasmOpcode::I64Extend16S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64((last.get_i64() as i16) as i64);
                }
                WasmOpcode::I64Extend32S => {
                    let last = value_stack
                        .last_mut()
                        .ok_or(WasmRuntimeError::InternalInconsistency)?;
                    *last = WasmStackValue::from_i64((last.get_i64() as i32) as i64);
                }

                _ => return Err(WasmRuntimeError::InvalidBytecode),
            }
        }
        if let Some(result_type) = result_types.first() {
            let val = value_stack
                .pop()
                .ok_or(WasmRuntimeError::InternalInconsistency)?;
            match result_type {
                WasmValType::I32 => Ok(WasmValue::I32(val.get_i32())),
                WasmValType::I64 => Ok(WasmValue::I64(val.get_i64())),
                // WasmValType::F32 => {}
                // WasmValType::F64 => {}
                _ => Err(WasmRuntimeError::InvalidParameter),
            }
        } else {
            Ok(WasmValue::Empty)
        }
    }

    fn branch(
        target: usize,
        block_stack: &mut Vec<usize>,
        value_stack: &mut Vec<WasmStackValue>,
        code_block: &mut WasmCodeBlock,
    ) -> Result<(), WasmRuntimeError> {
        block_stack.resize(block_stack.len() - target, 0);
        let block_position = block_stack
            .pop()
            .ok_or(WasmRuntimeError::InternalInconsistency)?;
        let block = code_block
            .info()
            .block_info(block_position)
            .ok_or(WasmRuntimeError::InternalInconsistency)?;

        let block_type = block.block_type;
        let new_len = block.stack_level;
        let new_position = block.preferred_target();
        if block_type == WasmBlockType::Empty {
            value_stack.resize(new_len, WasmStackValue::from_usize(0));
        } else {
            let top_val = value_stack
                .pop()
                .ok_or(WasmRuntimeError::InternalInconsistency)?;

            value_stack.resize(new_len, WasmStackValue::from_usize(0));
            value_stack.push(top_val);
        }
        code_block.set_position(new_position);
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub union WasmStackValue {
    i32: i32,
    u32: u32,
    i64: i64,
    u64: u64,
    f32: f32,
    f64: f64,
    usize: usize,
}

impl WasmStackValue {
    #[inline]
    pub const fn from_bool(v: bool) -> Self {
        if v {
            Self::from_usize(1)
        } else {
            Self::from_usize(0)
        }
    }

    #[inline]
    pub const fn from_usize(v: usize) -> Self {
        Self { usize: v as usize }
    }

    #[inline]
    pub const fn from_i32(v: i32) -> Self {
        Self { i64: v as i64 }
    }

    #[inline]
    pub const fn from_u32(v: u32) -> Self {
        Self { u64: v as u64 }
    }

    #[inline]
    pub const fn from_i64(v: i64) -> Self {
        Self { i64: v }
    }

    #[inline]
    pub const fn from_u64(v: u64) -> Self {
        Self { u64: v }
    }

    #[inline]
    pub fn get_bool(&self) -> bool {
        unsafe { self.i32 != 0 }
    }

    #[inline]
    pub fn get_i32(&self) -> i32 {
        unsafe { self.i32 }
    }

    #[inline]
    pub fn get_u32(&self) -> u32 {
        unsafe { self.u32 }
    }

    #[inline]
    pub fn get_i64(&self) -> i64 {
        unsafe { self.i64 }
    }

    #[inline]
    pub fn get_u64(&self) -> u64 {
        unsafe { self.u64 }
    }

    #[inline]
    pub fn get_f32(&self) -> f32 {
        unsafe { self.f32 }
    }

    #[inline]
    pub fn get_f64(&self) -> f64 {
        unsafe { self.f64 }
    }

    #[inline]
    pub fn get_u8(&self) -> u8 {
        unsafe { self.usize as u8 }
    }

    #[inline]
    pub fn get_u16(&self) -> u16 {
        unsafe { self.usize as u16 }
    }

    pub fn get_by_type(&self, val_type: WasmValType) -> WasmValue {
        match val_type {
            WasmValType::I32 => WasmValue::I32(self.get_i32()),
            WasmValType::I64 => WasmValue::I64(self.get_i64()),
            // WasmValType::F32 => {}
            // WasmValType::F64 => {}
            _ => todo!(),
        }
    }
}

impl From<bool> for WasmStackValue {
    fn from(v: bool) -> Self {
        Self::from_bool(v)
    }
}

impl From<usize> for WasmStackValue {
    fn from(v: usize) -> Self {
        Self::from_usize(v)
    }
}

impl From<u32> for WasmStackValue {
    fn from(v: u32) -> Self {
        Self::from_u32(v)
    }
}

impl From<i32> for WasmStackValue {
    fn from(v: i32) -> Self {
        Self::from_i32(v)
    }
}

impl From<u64> for WasmStackValue {
    fn from(v: u64) -> Self {
        Self::from_u64(v)
    }
}

impl From<i64> for WasmStackValue {
    fn from(v: i64) -> Self {
        Self::from_i64(v)
    }
}

impl From<WasmValue> for WasmStackValue {
    fn from(v: WasmValue) -> Self {
        match v {
            WasmValue::Empty => Self::from_i64(0),
            WasmValue::I32(v) => Self::from_i64(v as i64),
            WasmValue::I64(v) => Self::from_i64(v),
            _ => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WasmInterpreter;
    use crate::wasm::{Leb128Stream, WasmBlockInfo, WasmModule, WasmValType};

    #[test]
    fn add() {
        let slice = [0x20, 0, 0x20, 1, 0x6A, 0x0B];
        let local_types = [WasmValType::I32, WasmValType::I32];
        let result_types = [WasmValType::I32];
        let mut stream = Leb128Stream::from_slice(&slice);
        let module = WasmModule::new();
        let block_info =
            WasmBlockInfo::analyze(&mut stream, &local_types, &result_types, &module).unwrap();
        let mut code_block = super::WasmCodeBlock::from_slice(&slice, &block_info);

        let mut params = [1234.into(), 5678.into()];
        let result = WasmInterpreter::run(&mut code_block, &mut params, &result_types, &module)
            .unwrap()
            .get_i32()
            .unwrap();
        assert_eq!(result, 6912);

        let mut params = [0xDEADBEEFu32.into(), 0x55555555.into()];
        let result = WasmInterpreter::run(&mut code_block, &mut params, &result_types, &module)
            .unwrap()
            .get_i32()
            .unwrap();
        assert_eq!(result, 0x34031444);
    }

    #[test]
    fn sub() {
        let slice = [0x20, 0, 0x20, 1, 0x6B, 0x0B];
        let local_types = [WasmValType::I32, WasmValType::I32];
        let result_types = [WasmValType::I32];
        let mut stream = Leb128Stream::from_slice(&slice);
        let module = WasmModule::new();
        let block_info =
            WasmBlockInfo::analyze(&mut stream, &local_types, &result_types, &module).unwrap();
        let mut code_block = super::WasmCodeBlock::from_slice(&slice, &block_info);

        let mut params = [1234.into(), 5678.into()];
        let result = WasmInterpreter::run(&mut code_block, &mut params, &result_types, &module)
            .unwrap()
            .get_i32()
            .unwrap();
        assert_eq!(result, -4444);

        let mut params = [0x55555555.into(), 0xDEADBEEFu32.into()];
        let result = WasmInterpreter::run(&mut code_block, &mut params, &result_types, &module)
            .unwrap()
            .get_i32()
            .unwrap();
        assert_eq!(result, 0x76a79666);
    }

    #[test]
    fn loop_test() {
        let slice = [
            0x41, 0x00, 0x21, 0x01, 0x03, 0x40, 0x20, 0x01, 0x41, 0x01, 0x6a, 0x21, 0x01, 0x20,
            0x00, 0x41, 0x01, 0x6b, 0x22, 0x00, 0x0d, 0x00, 0x0b, 0x20, 0x01, 0x0b,
        ];
        let local_types = [WasmValType::I32, WasmValType::I32];
        let result_types = [WasmValType::I32];
        let mut stream = Leb128Stream::from_slice(&slice);
        let module = WasmModule::new();
        let block_info =
            WasmBlockInfo::analyze(&mut stream, &local_types, &result_types, &module).unwrap();
        let mut code_block = super::WasmCodeBlock::from_slice(&slice, &block_info);

        let mut params = [42.into(), 0.into()];
        let result = WasmInterpreter::run(&mut code_block, &mut params, &result_types, &module)
            .unwrap()
            .get_i32()
            .unwrap();
        assert_eq!(result, 42);
    }
}