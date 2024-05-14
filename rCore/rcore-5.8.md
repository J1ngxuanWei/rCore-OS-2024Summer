# rcore-5.8

## 做ch4

这个内核和应用的栈、地址空间，再一次看真的快被绕晕了

### 编程1:重写

为什么会失效？问题就在于提示所给出的

这个参数的结构是存在内存里的，需要更换，且可能会被两个页分割。

因为其两个都相当于去修改一个地址上的东西，而之前我们直接解引用就可以了，现在我们拿到的地址是虚拟地址，我们需要将其转换为物理地址后，修改物理地址上的东西。

为什么呢，因为我们这个参数，是一路从应用程序的代码那里传下来的，因此是必然要进行一次转换的。

转换的过程如下：

```rust
let us = get_time_us();
    let off: u32 = (_ts as u32) & 0xfff;
    let page_table = from_token(current_user_token());
    unsafe {
        if let Some(pte) = from_virnum(&page_table, VirtPageNum::from((_ts as usize))) {
            let ppn = pte.ppn();
            let mut tt: usize = ppn.into();
            tt = tt | off as usize;
            let mut tf: *mut TimeVal = tt as *mut TimeVal;
```

其中兜兜转转从虚拟地址，截取页内偏移，然后拿虚拟页号得到物理页号，最后把偏移加回去，然后解引用就行了。

重写的另一个也是这样的，没啥太大区别。













