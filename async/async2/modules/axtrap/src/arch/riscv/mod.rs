#[macro_use]
mod macros;

use alloc::boxed::Box;
use axhal::arch::TrapFrame;

use riscv::register::scause::{self, Exception as E, Trap};
use riscv::register::stvec;

use crate::trap::*;

#[cfg(feature = "monolithic")]
use linux_syscall_api::trap::MappingFlags;

include_trap_asm_marcos!();
core::arch::global_asm!(
    include_str!("trap.S"),
    trapframe_size = const core::mem::size_of::<TrapFrame>(),
);

extern "C" {
    fn __trap_from_user();
    fn __trap_from_kernel();
}

/// To initialize the trap vector base address.
pub fn init_interrupt() {
    set_kernel_trap_entry();
}

pub fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(__trap_from_kernel as usize, stvec::TrapMode::Direct);
    }
}

pub fn set_user_trap_entry() {
    unsafe {
        stvec::write(__trap_from_user as usize, stvec::TrapMode::Direct);
    }
}

#[no_mangle]
extern "C" fn task_entry() {
    // release the lock that was implicitly held across the reschedule
    axlog::warn!("enter task_entry,but is wrong");
    unsafe { axtask::RUN_QUEUE.force_unlock() };
    let task = axtask::current();
    let mut tf = task.get_tf();
    if let Some(entry) = task.get_entry() {
        if task.is_kernel_task() {
            // 是初始调度进程，直接执行即可
            unsafe { Box::from_raw(entry)() };
            // 继续执行对应的函数
        } else {
            // 进入到对应的应用程序
            loop {
                // 切换页表已经在switch实现了
                // 记得更新时间
                task.inner
                    .lock()
                    .time_stat_from_kernel_to_user(axhal::time::current_time_nanos() as usize);
                // return to user space
                riscv_trap_return(&mut tf);
                // next time when user traps into kernel, it will come back here
                riscv_trap_handler(&mut tf, true);

                task.set_tf(tf);
            }
        }
    }
    // only for kernel task
    axtask::exit(0);
}

#[no_mangle]
extern "C" fn riscv_trap_return(tf: &mut TrapFrame) {
    set_user_trap_entry();
    axhal::arch::disable_irqs();
    axhal::arch::flush_tlb(None);

    extern "C" {
        fn __return_to_user(cx: *mut TrapFrame);
    }

    unsafe {
        __return_to_user(tf);
    }
}

fn handle_breakpoint(sepc: &mut usize) {
    axlog::debug!("Exception(Breakpoint) @ {:#x} ", sepc);
    *sepc += 2
}

#[no_mangle]
extern "C" fn riscv_trap_test() {
    axlog::warn!("trap_test");
}

#[no_mangle]
extern "C" fn riscv_trap_handler(tf: &mut TrapFrame, _from_user: bool) {
    // 这个函数是给用户程序用的，kernel的trap有额外的处理
    set_kernel_trap_entry();
    let scause = scause::read();
    #[cfg(feature = "monolithic")]
    linux_syscall_api::trap::record_trap(scause.code());
    //axlog::warn!("user scause:{:?}", scause.cause());
    match scause.cause() {
        Trap::Exception(E::Breakpoint) => handle_breakpoint(&mut tf.sepc),
        Trap::Interrupt(_) => {
            //axlog::warn!("user scause:{:?}", scause.cause());
            handle_irq(scause.bits(), false)
        }

        #[cfg(feature = "monolithic")]
        Trap::Exception(E::UserEnvCall) => {
            axhal::arch::enable_irqs();
            //axlog::warn!("trap_handler :syscall id:{}", tf.regs.a7);
            tf.sepc += 4;
            let result = handle_syscall(
                tf.regs.a7,
                [
                    tf.regs.a0, tf.regs.a1, tf.regs.a2, tf.regs.a3, tf.regs.a4, tf.regs.a5,
                ],
            );
            tf.regs.a0 = result as usize;
            axhal::arch::disable_irqs();
        }

        #[cfg(feature = "monolithic")]
        Trap::Exception(E::InstructionPageFault) => {
            let addr = riscv::register::stval::read();
            handle_page_fault(addr.into(), MappingFlags::USER | MappingFlags::EXECUTE);
        }

        #[cfg(feature = "monolithic")]
        Trap::Exception(E::LoadPageFault) => {
            let addr = riscv::register::stval::read();
            handle_page_fault(addr.into(), MappingFlags::USER | MappingFlags::READ);
        }

        #[cfg(feature = "monolithic")]
        Trap::Exception(E::StorePageFault) => {
            let addr = riscv::register::stval::read();
            handle_page_fault(addr.into(), MappingFlags::USER | MappingFlags::WRITE);
        }

        _ => {
            panic!(
                "Unhandled user trap {:?} @ {:#x}:\n{:#x?}",
                scause.cause(),
                tf.sepc,
                tf
            );
        }
    }

    #[cfg(feature = "monolithic")]
    {
        //handle_signals();
    }
    //axlog::ax_println!("trap handle end");
}

/// Kernel trap handler
#[no_mangle]
pub fn riscv_kernel_trap_handler() {
    axlog::warn!("kernel trap start");
    let scause = scause::read();
    axlog::ax_println!("user scause:{:?},bits:{}", scause.cause(), scause.bits());
    match scause.cause() {
        Trap::Interrupt(_) => handle_irq(scause.bits(), false),
        _ => {
            axlog::ax_println!("fail scause:{:?}", scause.cause());
            panic!("Unhandled kernel trap {:?}", scause.cause(),);
        }
    }
    axlog::ax_println!("kernel trap end");
}
