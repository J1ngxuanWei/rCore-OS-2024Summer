# rcore-5.6



### 代码语法：

```rust
#[no_mangle]
extern "C" fn _start() {
    loop{};
}


#[no_mangle]: 这是一个属性（attribute），用于告诉编译器不要修改函数名。
在这里，它告诉编译器不要修改 _start 函数的名称，以便在后面的链接过程中能够正确地引用它。

extern "C" fn _start(): 这声明了一个外部函数 _start，它使用 C 调用约定（calling convention）。
这意味着 _start 函数的调用约定将按照 C 语言的标准来执行，这在与其他语言进行交互时很有用。

```

### 用户态执行环境

本质上需要一个合法的入口地址和执行一个合法的退出操作。

如果没有退出，会直接默认继续访问内存导致段错误。

### 用户态环境

本质上还是执行了物理机的os的系统调用，在内联汇编中通过将id调用号保存到相应的寄存器中，使用`ecall`命令来执行物理机的操作。

### 代码语法

```rust
#[no_mangle]
pub fn rust_main() -> ! {
    shutdown();
}

pub fn rust_main() -> ! {: 这是函数的声明。pub 关键字表示这个函数是公共的，可以被其他模块访问。
fn rust_main() -> ! 表示定义了一个名为 rust_main 的函数，它不返回任何值
（! 表示 "never" 类型，表示这个函数永远不会正常返回）。
```

### 代码语法

```rust
fn clear_bss() {
     extern "C" {
         fn sbss();
         fn ebss();
     }
     (sbss as usize..ebss as usize).for_each(|a| {
         unsafe { (a as *mut u8).write_volatile(0) }
     });
}

fn clear_bss() {: 这是一个函数定义，名为 clear_bss。

extern "C" { ... }: 这是一个 extern 块，用于声明外部函数。在这里，它声明了两个外部函数：sbss() 和 ebss()。
这些函数通常是由链接器生成的，分别表示 BSS 段的起始地址和结束地址。

(sbss as usize..ebss as usize): 这是一个范围表达式，表示从 sbss 到 ebss 之间的地址范围，
其中 sbss 和 ebss 是两个函数的地址。

.for_each(|a| { ... });: 这是一个迭代器方法，对上面定义的范围中的每一个元素执行特定的操作。
在这里，对每一个地址 a 执行一个闭包中的操作。

unsafe { (a as *mut u8).write_volatile(0) }: 这行代码是不安全的，因为它直接操作了内存地址，
并且使用了 write_volatile 方法，表示写入内存时不做优化，确保写入立即生效。
它将地址 a 强制转换为 *mut u8 类型的指针，然后写入一个值为0的字节到这个地址。
```

### 代码语法

```rust
lazy_static! {
    static ref APP_MANAGER: UPSafeCell<AppManager> = unsafe {
        UPSafeCell::new({
            extern "C" {
                fn _num_app();
            }
            let num_app_ptr = _num_app as usize as *const usize;
            let num_app = num_app_ptr.read_volatile();
            let mut app_start: [usize; MAX_APP_NUM + 1] = [0; MAX_APP_NUM + 1];
            let app_start_raw: &[usize] =
                core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1);
            app_start[..=num_app].copy_from_slice(app_start_raw);
            AppManager {
                num_app,
                current_app: 0,
                app_start,
            }
        })
    };
}

lazy_static! { ... }: 这是 Rust 的 lazy_static 宏的调用，用于创建一个懒加载的静态变量。
这意味着变量的初始化将在第一次被访问时进行。

static ref APP_MANAGER: UPSafeCell<AppManager> = ...: 这声明了一个静态变量 APP_MANAGER，
类型为 UPSafeCell<AppManager>。UPSafeCell 是一个自定义的类型，可能是为了实现更加安全的并发访问。

unsafe { ... }: 这是一个不安全块，里面包含了对不安全操作的调用。在这里，
它是对 UPSafeCell 的 new 方法的调用。

UPSafeCell::new({ ... }): 这是调用 UPSafeCell 的 new 方法，
用于创建一个新的 UPSafeCell 实例。在这里，它接受一个闭包作为参数，
闭包中包含了变量的初始化逻辑。

extern "C" { ... }: 这是声明了一个外部函数，_num_app，
它是一个 C 语言中的函数，可能由链接器生成。

let num_app_ptr = _num_app as usize as *const usize;: 
这一行将外部函数 _num_app 的地址转换为 *const usize 类型的指针，并存储在 num_app_ptr 变量中。

let num_app = num_app_ptr.read_volatile();: 这一行从 num_app_ptr 指向的地址读取一个 usize 类型的值，
并使用 read_volatile 方法，确保立即读取该值而不做任何优化。

接下来的代码块，初始化了一个类型为 AppManager 的变量，并将其包含的字段填充为相应的值。
这部分的具体逻辑包括从外部函数读取数据、创建数组，并将数据从一个位置复制到另一个位置。
```




