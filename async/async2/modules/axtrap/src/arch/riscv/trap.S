.macro SAVE_REGS
    addi    sp, sp, -{trapframe_size}
    PUSH_GENERAL_REGS
.endm

.macro RESTORE_REGS
    POP_GENERAL_REGS
    addi    sp, sp, {trapframe_size}
.endm

.section .text
.balign 4
.global __trap_from_user
__trap_from_user:
    csrrw   sp, sscratch, sp            // switch sscratch and sp
    # now sp->*TrapFrame in kernel space, sscratch->user stack
    # save other general purpose registers
    PUSH_GENERAL_REGS
    
    csrr    t0, sepc
    csrr    t1, sstatus
    STR     t0, sp, 31                  // tf.sepc
    STR     t1, sp, 32                  // tf.sstatus
    # read user stack from sscratch and save it in TrapContext
    csrr    t2, sscratch
    STR     t2, sp, 1                   // tf.regs.sp
    .short  0xa622                      // fsd fs0,264(sp)
    .short  0xaa26                      // fsd fs1,272(sp)
    

    #下面的目前不需要用，后续看运行的情况再说
    #LDR     t1, sp, 2                   // load user gp with CPU ID
    #LDR     t0, sp, 3                   // load supervisor tp
    #STR     gp, sp, 2                   // save user gp and tp
    #STR     tp, sp, 3
    #mv      gp, t1
    #mv      tp, t0

    #call riscv_trap_test

    # # move to kernel_sp
    # load kernel ra
    LDR ra, sp, 35
    # load callee-saved regs
    LDR s0, sp, 37
    LDR s1, sp, 38
    LDR s2, sp, 39
    LDR s3, sp, 40
    LDR s4, sp, 41
    LDR s5, sp, 42
    LDR s6, sp, 43
    LDR s7, sp, 44
    LDR s8, sp, 45
    LDR s9, sp, 46
    LDR s10, sp, 47
    LDR s11, sp, 48
    LDR tp, sp, 49
    LDR sp, sp, 36
    # return to kernel ra
    ret


.global __trap_from_kernel
__trap_from_kernel:
    SAVE_REGS
    call    riscv_kernel_trap_handler
    RESTORE_REGS
    sret

.globl __return_to_user
__return_to_user:
    # a0: *TrapFrame in user space(Constant); a1: user space token
    # switch to user space
    # let sscratch store the trap context's address
    csrw sscratch, a0
    # save kernel callee-saved regs
    STR sp, a0, 36
    STR ra, a0, 35
    STR s0, a0, 37
    STR s1, a0, 38
    STR s2, a0, 39
    STR s3, a0, 40
    STR s4, a0, 41
    STR s5, a0, 42
    STR s6, a0, 43
    STR s7, a0, 44
    STR s8, a0, 45
    STR s9, a0, 46
    STR s10, a0, 47
    STR s11, a0, 48
    STR tp, a0, 49

    #下面的目前不需要用，后续看运行的情况再说
    #for tp and gp
    #LDR     t0, sp, 2                   
    #STR     gp, sp, 2                  // gp
    #mv      gp, t0
    #LDR     t0, sp, 3
    #STR     tp, sp, 3                   // save supervisor tp. Note that it is stored on the kernel stack rather than in sp, in which case the ID of the currently running CPU should be stored
    #mv      tp, t0                      // tp: now it stores the TLS pointer to the corresponding thread

    mv sp, a0
    # now sp points to TrapFrame in kernel space, start restoring based on it
    # restore sstatus/sepc
    LDR     t0, sp, 31
    LDR     t1, sp, 32
    csrw    sepc, t0
    csrw    sstatus, t1
    # restore general purpose registers
    POP_GENERAL_REGS
    # back to user stack
    LDR     sp, sp, 1
    sret

