use riscv::register::sstatus::{self, Sstatus};
include_asm_marcos!();

/// General registers of RISC-V.
#[allow(missing_docs)]
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct GeneralRegisters {
    pub ra: usize,
    pub sp: usize,
    pub gp: usize, // only valid for user traps
    pub tp: usize, // only valid for user traps
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
}

/// Saved registers when a trap (interrupt or exception) occurs.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    /// All general registers.
    pub regs: GeneralRegisters,
    /// Supervisor Exception Program Counter.
    pub sepc: usize,      //31
    /// Supervisor Status Register.
    pub sstatus: usize,   //32
    /// 浮点数寄存器
    pub fs: [usize; 2],   //33,34
    /// trap for kernel
    pub kernel_ra: usize, // 35
    pub kernel_sp: usize, // 36

    pub kernel_s0: usize, // 37
    pub kernel_s1: usize, // 38

    pub kernel_s2: usize, // 39
    pub kernel_s3: usize, // 40
    pub kernel_s4: usize, // 41
    pub kernel_s5: usize, // 42
    pub kernel_s6: usize, // 43
    pub kernel_s7: usize, // 44
    pub kernel_s8: usize, // 45
    pub kernel_s9: usize, // 46
    pub kernel_s10: usize,// 47
    pub kernel_s11: usize,// 48

    pub kernel_tp: usize, // 49
}

impl TrapFrame {
    pub fn set_user_sp(&mut self, user_sp: usize) {
        self.regs.sp = user_sp;
    }

    /// 用于第一次进入应用程序时的初始化
    pub fn app_init_context(app_entry: usize, user_sp: usize) -> Self {
        let sstatus = sstatus::read();
        // 当前版本的riscv不支持使用set_spp函数，需要手动修改
        // 修改当前的sstatus为User，即是第8位置0
        let mut trap_frame = TrapFrame::default();
        trap_frame.set_user_sp(user_sp);
        trap_frame.sepc = app_entry;
        trap_frame.sstatus =
            unsafe { (*(&sstatus as *const Sstatus as *const usize) & !(1 << 8)) & !(1 << 1) };
        unsafe {
            // a0为参数个数
            // a1存储的是用户栈底，即argv
            trap_frame.regs.a0 = *(user_sp as *const usize);
            trap_frame.regs.a1 = *(user_sp as *const usize).add(1);
        }
        trap_frame
    }

    /// 设置返回值
    pub fn set_ret_code(&mut self, ret_value: usize) {
        self.regs.a0 = ret_value;
    }

    /// 设置TLS
    pub fn set_tls(&mut self, tls_value: usize) {
        self.regs.tp = tls_value;
    }

    /// 获取 sp
    pub fn get_sp(&self) -> usize {
        self.regs.sp
    }

    /// 设置 pc
    pub fn set_pc(&mut self, pc: usize) {
        self.sepc = pc;
    }

    /// 设置 arg0
    pub fn set_arg0(&mut self, arg: usize) {
        self.regs.a0 = arg;
    }

    /// 设置 arg1
    pub fn set_arg1(&mut self, arg: usize) {
        self.regs.a1 = arg;
    }

    /// 设置 arg2
    pub fn set_arg2(&mut self, arg: usize) {
        self.regs.a2 = arg;
    }

    /// 获取 pc
    pub fn get_pc(&self) -> usize {
        self.sepc
    }

    /// 获取 ret
    pub fn get_ret_code(&self) -> usize {
        self.regs.a0
    }

    /// 设置返回地址
    pub fn set_ra(&mut self, ra: usize) {
        self.regs.ra = ra;
    }

    /// 获取所有 syscall 参数
    pub fn get_syscall_args(&self) -> [usize; 6] {
        [
            self.regs.a0,
            self.regs.a1,
            self.regs.a2,
            self.regs.a3,
            self.regs.a4,
            self.regs.a5,
        ]
    }

    /// 获取 syscall id
    pub fn get_syscall_num(&self) -> usize {
        self.regs.a7 as _
    }

    pub fn set_ss(&mut self, ss: usize) {
        self.sstatus = ss;
    }

    pub fn load_new(&mut self, tf: &TrapFrame) {
        self.kernel_ra = tf.kernel_ra;
        self.kernel_sp = tf.kernel_sp;
        self.kernel_s0 = tf.kernel_s0;
        self.kernel_s1 = tf.kernel_s1;
        self.kernel_s2 = tf.kernel_s2;
        self.kernel_s3 = tf.kernel_s3;
        self.kernel_s4 = tf.kernel_s4;
        self.kernel_s5 = tf.kernel_s5;
        self.kernel_s6 = tf.kernel_s6;
        self.kernel_s7 = tf.kernel_s7;
        self.kernel_s8 = tf.kernel_s8;
        self.kernel_s9 = tf.kernel_s9;
        self.kernel_s10 = tf.kernel_s10;
        self.kernel_s11 = tf.kernel_s11;
        self.kernel_tp = tf.kernel_tp;
        self.regs = tf.regs;
        self.sepc = tf.sepc;
        self.sstatus = tf.sstatus;
        self.fs = tf.fs;
    }

    pub fn store_old(&self,tf:&mut TrapFrame){
        tf.kernel_ra = self.kernel_ra;
        tf.kernel_sp = self.kernel_sp;
        tf.kernel_s0 = self.kernel_s0;
        tf.kernel_s1 = self.kernel_s1;
        tf.kernel_s2 = self.kernel_s2;
        tf.kernel_s3 = self.kernel_s3;
        tf.kernel_s4 = self.kernel_s4;
        tf.kernel_s5 = self.kernel_s5;
        tf.kernel_s6 = self.kernel_s6;
        tf.kernel_s7 = self.kernel_s7;
        tf.kernel_s8 = self.kernel_s8;
        tf.kernel_s9 = self.kernel_s9;
        tf.kernel_s10 = self.kernel_s10;
        tf.kernel_s11 = self.kernel_s11;
        tf.kernel_tp = self.kernel_tp;
        tf.regs = self.regs;
        tf.sepc = self.sepc;
        tf.sstatus = self.sstatus;
        tf.fs = self.fs;
    }
}

#[no_mangle]
#[cfg(feature = "monolithic")]
/// To handle the first time into the user space
///
/// 1. push the given trap frame into the kernel stack
/// 2. go into the user space
///
/// args:
///
/// 1. kernel_sp: the top of the kernel stack
///
/// 2. frame_base: the address of the trap frame which will be pushed into the kernel stack
pub fn first_into_user(_kernel_sp: usize) {
    // Make sure that all csr registers are stored before enable the interrupt
    //已经不再需要这个函数
    //已经转移到axtrap中了
}


#[allow(unused)]
/// To switch the context between two tasks
pub fn task_context_switch(next_ctx: &TrapFrame) {
    axlog::warn!("ta1");
    unsafe {
        // 打印出要恢复的寄存器值以进行调试
        core::arch::asm!(
            "
            // restore new context
            mv      sp, {tsp}
            ",
            tsp = in(reg) next_ctx.kernel_sp,
        );
    }
    axlog::warn!("ta2");
}
