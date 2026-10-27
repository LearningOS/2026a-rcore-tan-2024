# 第五章实验报告：进程及进程管理

## 一、实现功能总结

迁移 `sys_get_time`、`sys_mmap` 和 `sys_munmap`，使其适应新的进程结构。实现 `sys_spawn`，支持根据应用名称创建子进程并执行目标程序。为任务控制块增加优先级和 stride 状态，实现 `sys_set_priority`，并将原有 FIFO 调度改为 Stride 调度：每次选择 stride 最小的就绪进程，根据优先级更新其 stride，同时处理整数溢出比较问题。

## 二、问答作业

### 1. Stride 溢出时，实际会轮到进程 P1 执行吗？为什么？

不会。如果直接使用普通整数大小比较，可能无法正确选择理论上 stride 最小的进程。

例如，使用 8 位无符号整数存储 stride，两个进程的步长均为 10，当前有：

- P1：`stride = 255`
- P2：`stride = 250`

P2 执行一个时间片后，其 stride 理论上变为 260，但 8 位无符号整数溢出后得到 4。

此时，普通数值比较认为 P2 的 stride 为 4，小于 P1 的 255，因此仍然选择 P2，而不是理论上应当执行的 P1。

这是因为 stride 实际上具有循环计数的特征，溢出后的数值不能直接按照普通整数的大小关系进行比较，需要使用能够识别回绕关系的比较方法。

### 2. 为什么在进程优先级均不小于 2 时，可以得到 STRIDE_MAX − STRIDE_MIN 不超过 BigStride / 2？

设每个进程的步长为：

`pass = BigStride / priority`

由于所有进程的优先级均不小于 2，因此每个进程的步长满足：

`pass ≤ BigStride / 2`

假设当前所有进程的 stride 均位于区间 `[m, m + BigStride / 2]`，其中 `m` 为最小 stride。

调度器每次选择 stride 最小的进程，并将其 stride 增加对应的步长。由于增加量不超过 `BigStride / 2`，被调度进程更新后的 stride 仍然不会超过 `m + BigStride / 2`。其他进程的 stride 保持不变，也仍位于该区间内。

初始时所有进程的 stride 均为 0，显然满足上述条件。因此，在不考虑整数溢出的情况下，通过归纳可知，每次调度后所有进程的 stride 之差均不超过 `BigStride / 2`。

这一界限说明，只要正确处理溢出，并使比较所依据的数值差处于可区分的范围内，就能够恢复 stride 的正确顺序。

### 3. 如何设计能够正确处理溢出的 Stride 比较器？

对于 `u64` 类型，可以利用无符号整数的环绕减法判断两个 stride 的相对顺序。由于 `u64` 的取值范围为 (0) 到 (2^{64}-1)，其模数为 (2^{64})，因此可以使用 (2^{63}) 作为区分两个方向的半区间边界。

在题目假设两个 stride 不相等的情况下，`partial_cmp` 可以实现为：

```rust
use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let distance = other.0.wrapping_sub(self.0);

        if distance < (1u64 << 63) {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Greater)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}
```

其中，`wrapping_sub` 可以在减法溢出时保留环绕后的结果。如果 `other - self` 的环绕结果小于 (2^{63})，则认为 `self` 小于 `other`；否则认为 `self` 大于 `other`。

例如，对于 8 位无符号整数，按照模 256 计算：

- 比较 125 和 255 时，`255 - 125 = 130`，超过半区间 128，因此判断 `125` 不小于 `255`。
- 比较 129 和 255 时，`255 - 129 = 126`，小于半区间 128，因此判断 `129` 小于 `255`。

这样就可以在满足 stride 差值约束的前提下，正确识别整数回绕前后的相对顺序，避免普通数值比较导致调度错误。

## 三、实验测试

### 1. 用户态综合测试

在 Docker 环境中执行：

```bash
make -C os run BASE=2
```

进入 rCore Shell 后运行：

```text
ch5_usertest
```

测试日志显示 `ch5 Usertests passed!`。其中，`ch5_spawn0`、`ch5_spawn1`、`ch5_setprio` 和 `ch5_stride` 均显示退出码为 0，说明这些测试程序正常结束。

### 2. 官方检查器测试

运行第五章官方检查器：

```bash
make -C ci-user test CHAPTER=5
```

此前一次检查的功能匹配结果为：

```text
Test passed50244: 15/15
```

其中，`spawn`、`set_priority`、`get_time`、`mmap`、`munmap` 及前面章节的兼容性检查均通过。

## 四、荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我未曾和其他对象做过交流。

2. 此外，我也参考了以下资料，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   [rCore-Tutorial-Guide 第五章练习](https://learningos.cn/rCore-Tutorial-Guide/chapter5/4exercise.html)：用于确认 `sys_get_time`、`sys_mmap`、`sys_munmap`、`sys_spawn`、`sys_set_priority` 的接口规范，以及 Stride 调度算法、优先级约束和溢出比较问题。

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
