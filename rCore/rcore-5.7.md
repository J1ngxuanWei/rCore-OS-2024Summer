# rcore-5.7

## 做ch3

### 编程任务

实现一个系统调用

查询当前正在执行的任务信息，任务信息包括任务控制块相关信息（任务状态）、任务使用的系统调用及调用次数、系统调用时刻距离任务第一次被调度时刻的时长（单位ms）。

任务内容就是查询一次，也就是检查一次。。我理解是这样的，有点迷，这个调用有啥用吗。。

首先设置运行状态，一定是running：

```rust
(*_ti).status = TaskStatus::Running;
```

然后我们需要统计任务的信息，保存其第一次被调用的时候的时刻和syscall次数表，我选择是在`TaskControlBlock`里面添加。

因为我们一个task，就是一个应用，它都有一个单独的task的控制结构，因此我们直接存这里面是没有问题的。

如下：

```rust
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// syscall times
    pub syscall_times: [u32; 500],
    /// first schedule time
    pub first_schedule_time: usize,
}
```

为什么挑这个呢，因为只需要初始化一次。。改的次数比较少。

```rust
let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::UnInit,
            syscall_times: [0; 500],
            first_schedule_time: 0,
        }; MAX_APP_NUM];
```

这样就可以了，然后我们在进入内核的S态的syscall分发函数处，添加一次调用

```rust
/// handle syscall exception with `syscall_id` and other arguments
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    add_syscall_times(syscall_id);
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_TASK_INFO => sys_task_info(args[0] as *mut TaskInfo),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
```

具体的实现如下：

```rust
//os/src/task/mod.rs
fn add_syscall_times(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_times[syscall_id] += 1;
    }

fn get_syscall_times(&self, syscall_id: usize) -> u32 {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_times[syscall_id]
    }
/// Add syscall times for current task
pub fn add_syscall_times(syscall_id: usize) {
    TASK_MANAGER.add_syscall_times(syscall_id);
}

/// Get syscall times for current task
pub fn get_syscall_times(syscall_id: usize) -> u32 {
    TASK_MANAGER.get_syscall_times(syscall_id)
}
```

然后我们调用时直接赋值过去就行了：

```rust
for i in 0..MAX_SYSCALL_NUM{
            (*_ti).syscall_times[i] = get_syscall_times(i);
        }
```

然后就完成了，对于time，也是一样的道理：

```rust
//os/src/task/mod.rs
task0.first_schedule_time = get_time_ms();

if inner.tasks[next].first_schedule_time == 0 {
                inner.tasks[next].first_schedule_time = get_time_ms();
            }

fn get_first_schedule_time(&self) -> usize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].first_schedule_time
    }            

/// Get first schedule time for current task
pub fn get_first_schedule_time() -> usize {
    TASK_MANAGER.get_first_schedule_time()
}
```

然后在调用处直接使用就可以：

```rust
(*_ti).time = get_time_ms()-get_first_schedule_time();
```    

随后通过全部样例。


















