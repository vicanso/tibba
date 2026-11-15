# tibba modules

Workspace 内 `tibba-*` crate 的 path 依赖关系（由 `cargo run --bin generate-mermaid` 生成）。

更新：

```bash
make mermaid
# 或
cargo run --bin generate-mermaid
```

```mermaid
graph TD
    config --> error

    crypto --> error

    runtime --> error

    util --> error

    cache --> config
    cache --> error
    cache --> util

    model --> config
    model --> crypto
    model --> error
    model --> util

    opendal --> config
    opendal --> error
    opendal --> util

    request --> error
    request --> util

    auth --> cache
    auth --> crypto
    auth --> error
    auth --> request
    auth --> runtime
    auth --> util

    middleware --> auth
    middleware --> cache
    middleware --> error
    middleware --> runtime
    middleware --> util

    router --> auth
    router --> cache
    router --> crypto
    router --> error
    router --> middleware
    router --> model
    router --> opendal
    router --> request
    router --> runtime
    router --> util
```