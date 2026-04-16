# tibba 项目编码规范

## 错误处理：snafu 模式

所有 `tibba-*` 模块必须使用 `snafu` 进行错误处理，禁止直接使用 `.map_err(|e| ...)` 包装外部错误。

### 规则

1. **每个模块定义模块级 `Error` enum**，每个外部错误来源对应一个 variant：
   ```rust
   #[derive(Debug, Snafu)]
   pub enum Error {
       #[snafu(display("... {source}"))]
       VariantName { source: ExternalErrorType },

       // 需要额外上下文字段时：
       #[snafu(display("... {field}: {source}"))]
       VariantWithContext { field: String, source: ExternalErrorType },

       // 无 source，仅有消息时：
       #[snafu(display("{message}"))]
       Invalid { message: String },
   }
   ```
   - 使用具体类型，不用 `Box<dyn Error>`
   - 大型 source 类型（≥128 字节）需装箱：`#[snafu(source(from(BigType, Box::new)))]`

2. **实现 `From<Error> for tibba_error::Error`**，参数命名用 `val`：
   ```rust
   impl From<Error> for tibba_error::Error {
       fn from(val: Error) -> Self {
           let err = match val {
               Error::Foo { source } => tibba_error::Error::new(source),
               Error::Bar { message } => tibba_error::Error::new(message),
           };
           err.with_category("<module-name>")
       }
   }
   ```

3. **调用侧用 `.context(XxxSnafu { ... })?`** 替代 `.map_err(...)`：
   ```rust
   // 禁止
   some_result.map_err(|e| Error::Foo { source: e })?;

   // 正确
   some_result.context(FooSnafu)?;
   some_result.context(BarSnafu { field: "value" })?;
   ```

4. **Result 类型别名**：
   - 模块内部：`type Result<T, E = Error> = std::result::Result<T, E>;`
   - 公开函数返回 `tibba_error::Error`，通过 `?` 自动调用 `From` 转换

5. **`Cargo.toml`** 确保包含 `snafu = { workspace = true }`

6. **不引入 snafu 的情形**：模块所有错误已经是 `tibba_error::Error`，无需包装

## 链式配置（Fluent Interface）

结构体有多个可选参数时，必须使用链式 `with_xxx()` / `add_xxx()` 方法，禁止在构造函数中堆砌参数。

### 规则

1. **构造函数只接收必填参数**，可选参数通过链式方法设置：
   ```rust
   // 禁止
   RedisCache::new(client, Some(ttl), Some("prefix:".to_string()))

   // 正确
   RedisCache::new(client)
       .with_ttl(Duration::from_secs(300))
       .with_prefix("prefix:")
   ```

2. **链式方法签名固定形式** — 消耗 `self` 并返回 `Self`：
   ```rust
   #[must_use]
   pub fn with_timeout(mut self, timeout: Duration) -> Self {
       self.timeout = Some(timeout);
       self
   }
   ```
   - 加 `#[must_use]`，避免调用方忘记接收返回值
   - 方法命名：设置单个字段用 `with_xxx`，追加集合元素用 `add_xxx`

3. **禁止使用 `&mut self` 返回 `()`** 的配置方法（不支持链式调用）：
   ```rust
   // 禁止
   pub fn with_stat_callback(&mut self, cb: &'static Fn()) {
       self.callback = Some(cb);
   }

   // 正确
   #[must_use]
   pub fn with_stat_callback(mut self, cb: &'static Fn()) -> Self {
       self.callback = Some(cb);
       self
   }
   ```

4. **参数类型尽量宽泛**，减少调用侧的 `.to_string()` 负担：
   ```rust
   // 禁止
   pub fn with_prefix(mut self, prefix: String) -> Self { ... }

   // 正确
   pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
       self.prefix = prefix.into();
       self
   }
   ```

5. **有 `with_xxx()` setter 的字段必须设为私有**（去掉 `pub`），强制调用方通过链式方法配置，防止绕过封装直接赋值：
   ```rust
   pub struct LimitParams {
       max: i64,        // 私有，只能通过 new(max) 设置
       category: String, // 私有，只能通过 with_category() 设置
       ttl: Duration,   // 私有，只能通过 with_ttl() 设置
   }
   ```

6. **复杂对象使用独立 Builder**，通过 `.build()` 最终构造：
   ```rust
   let client = ClientBuilder::new("service")
       .with_base_url("https://api.example.com")
       .with_timeout(Duration::from_secs(30))
       .with_connect_timeout(Duration::from_secs(5))
       .with_common_interceptor()
       .build()?;
   ```

### 项目内典型参考

| 类型 | 文件 | 链式方法示例 |
|------|------|-------------|
| `Error` | `tibba-error/src/lib.rs` | `with_category` / `with_status` / `add_extra` |
| `ClientBuilder` | `tibba-request/src/request.rs` | `with_base_url` / `with_timeout` / `with_interceptor` |
| `RedisCache` | `tibba-cache/src/cache.rs` | `with_ttl` / `with_prefix` |
| `Session` | `tibba-session/src/session.rs` | `with_account` / `with_roles` / `with_groups` |

## 其他约定

- 字符串初始化用 `String::new()` 而非 `"".to_string()`
- `From` impl 中解构 `val` 用单次 match，避免双重 match
- `impl Xxx { fn new(...) -> Self }` 返回类型统一用 `Self`
- 每次修改后运行 `cargo clippy -p <crate-name>` 确保零警告
