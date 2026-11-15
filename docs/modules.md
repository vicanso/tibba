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

    email --> error

    i18n --> error

    job --> error

    runtime --> error

    util --> error

    model --> crypto
    model --> error

    totp --> crypto

    cache --> config
    cache --> error
    cache --> util

    opendal --> config
    opendal --> error
    opendal --> util

    request --> error
    request --> util

    sql --> config
    sql --> error
    sql --> util

    model-builtin --> model

    model-token --> model

    feature --> cache
    feature --> error

    jwt --> cache
    jwt --> error
    jwt --> util

    session --> cache
    session --> error
    session --> runtime
    session --> util

    llm --> error
    llm --> request

    notify --> email
    notify --> error
    notify --> request

    oauth --> error
    oauth --> request

    webhook --> crypto
    webhook --> error
    webhook --> job
    webhook --> request

    middleware --> cache
    middleware --> error
    middleware --> runtime
    middleware --> session
    middleware --> util

    rbac --> cache
    rbac --> error
    rbac --> model-builtin
    rbac --> session

    router-common --> cache
    router-common --> error
    router-common --> runtime
    router-common --> session
    router-common --> util

    router-file --> error
    router-file --> model-builtin
    router-file --> opendal
    router-file --> session
    router-file --> util

    router-model --> error
    router-model --> model
    router-model --> runtime
    router-model --> session
    router-model --> util

    tenant --> error
    tenant --> session

    router-user --> cache
    router-user --> crypto
    router-user --> email
    router-user --> error
    router-user --> jwt
    router-user --> middleware
    router-user --> model
    router-user --> model-builtin
    router-user --> oauth
    router-user --> session
    router-user --> totp
    router-user --> util
```