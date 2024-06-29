use axhal::arch::TrapFrame;

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub ra: usize, // return address (x1)
    pub sp: usize, // stack pointer (x2)

    pub s0: usize, // x8-x9
    pub s1: usize,

    pub s2: usize, // x18-x27
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,

    pub tp: usize,
}

impl Context {
    pub fn store_old(&self, tf: &mut TrapFrame) {
        tf.kernel_ra = self.ra;
        tf.kernel_sp = self.sp;
        tf.kernel_s0 = self.s0;
        tf.kernel_s1 = self.s1;
        tf.kernel_s2 = self.s2;
        tf.kernel_s3 = self.s3;
        tf.kernel_s4 = self.s4;
        tf.kernel_s5 = self.s5;
        tf.kernel_s6 = self.s6;
        tf.kernel_s7 = self.s7;
        tf.kernel_s8 = self.s8;
        tf.kernel_s9 = self.s9;
        tf.kernel_s10 = self.s10;
        tf.kernel_s11 = self.s11;
        tf.kernel_tp = self.tp;
    }

    pub fn load_new(&mut self, tf: &TrapFrame) {
        self.ra = tf.kernel_ra;
        self.sp = tf.kernel_sp;
        self.s0 = tf.kernel_s0;
        self.s1 = tf.kernel_s1;
        self.s2 = tf.kernel_s2;
        self.s3 = tf.kernel_s3;
        self.s4 = tf.kernel_s4;
        self.s5 = tf.kernel_s5;
        self.s6 = tf.kernel_s6;
        self.s7 = tf.kernel_s7;
        self.s8 = tf.kernel_s8;
        self.s9 = tf.kernel_s9;
        self.s10 = tf.kernel_s10;
        self.s11 = tf.kernel_s11;
        self.tp = tf.kernel_tp;
    }
}
