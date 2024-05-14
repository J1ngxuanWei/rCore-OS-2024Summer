# rcore-5.9

写ch4遇到一个问题，拿到物理地址之后写不进去：

```rust
//println!("pte: {:?}", pte.bits);
            let ppn = pte.ppn();
            let mut tt: usize = ppn.into();
            tt=tt&0xfffff000;
            println!("tt: {:?}", tt);   
            tt = tt | off as usize;
            let mut tf: *mut TimeVal = tt as *mut TimeVal;
            println!("tf: {:?}", tf);
            *tf = TimeVal {
                sec: us / 1_000_000,
                usec: us % 1_000_000,
            };
```

上面的*tf写不进去。。我觉得应该是直接这样访问的问题，考虑尝试将其转换为一个个的字节，写字节的数据。

放弃了，，好像不能直接写。。必须要通过拿到vector后写vector，不理解。。。

### 问题2

0x1000000空间。。查了半天发现没有这个虚拟页

结果发现我tm去内核区域找了。。。草，我是傻逼

```rust
let mut kernel_space = KERNEL_SPACE.exclusive_access();
```

就是上面的罪魁祸首，应该去查用户栈


--------------------------------
已经放弃，，改废了，这rust真的折磨人

我之前的思路有点沿袭ucore的思路，因为ucore是用链表管理的，而且c的检查，，，懂的都懂，尽管大胆操作，不要在意bug。导致我一直想拿出某个内存区域来直接改。。。就寄了

rust的所有权的机制导致很难一层层的拿上来，然后再去修改，只能一层层的解耦合向下，到对应的结构体时修改成员变量。

需要反思。。。


