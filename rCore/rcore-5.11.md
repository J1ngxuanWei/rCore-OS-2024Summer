# rcore-5.11

## ch5编程1

实现比较简单，大部分函数框架都实现了，只需要结合exec和fork对比实现就可以了：

```rust
/// parent process fork the child process
    pub fn spawn(self: &Arc<Self>,elf_data: &[u8]) -> Arc<Self> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: parent_inner.heap_bottom,
                    program_brk: parent_inner.program_brk,
                    syscall_times: [0; 500],
                    first_schedule_time: 0,
                })
            },
        });

        // add child
        parent_inner.children.push(task_control_block.clone());
        // **** access child PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        // return
        task_control_block
        // **** release child PCB
        // ---- release parent PCB
    }
```

系统调用处，跟fork很类似，前面借鉴exec：

```rust
/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
#[allow(unused)]
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.spawn(data);
        let new_pid = new_task.pid.0;
        // modify trap context of new_task, because it returns immediately after switching
        let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
        // we do not have to move to next instruction since we have done it before
        // for child process, fork returns 0
        trap_cx.x[10] = 0;
        // add new task to scheduler
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}
```

## ch5编程2

调度的地方在于`run_task`这个函数，其一直在`loop`，不断的取出一个task来将其设置为RUNNING。

这样思路就比较清晰了，一路溯源到`TaskManager`的`fetch`处给出对应的task就行了。

首先在inner中添加对应的变量，然后初始化。

然后在对应的fetch里面修改就行了：

```rust
/// Take a process out of the ready queue
    #[allow(unused)]
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if let Some(task) = self.ready_queue.front() {
            let mut str: isize = 0;
            let mut str_sti = isize::MAX;
            for (i, task) in self.ready_queue.iter().enumerate() {
                if task.inner_exclusive_access().stride < str_sti {
                    str_sti = task.inner_exclusive_access().stride;
                    str = i as isize;
                }
            }
            for i in 0..1000 {
                if i == str {
                    break;
                }
                let taskk = self.ready_queue.pop_front().unwrap();
                self.ready_queue.push_back(taskk);
            }
            let mut taskk = self.ready_queue.pop_front().unwrap();
            let pro = taskk.inner_exclusive_access().priority;
            taskk.inner_exclusive_access().stride += 1000 / pro;
            Some(taskk)
        } else {
            None
        }
    }
```

其中大常数设置成了1000，过了样例。



