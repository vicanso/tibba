use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing_subscriber::Layer;

#[derive(Default, Debug)]
pub struct HttpTrace {
    pub start: AtomicU64,
    pub get_from_pool: AtomicU64,
    pub got_conn: AtomicU64,
    pub dns_start: AtomicU64,
    pub dns_done: AtomicU64,
    pub conn_start: AtomicU64,
    pub conn_done: AtomicU64,
    pub tls_start: AtomicU64,
    pub tls_done: AtomicU64,
    pub handshake_start: AtomicU64,
    pub handshake_done: AtomicU64,
    pub done: AtomicU64,
}

#[derive(Default, Debug)]
pub struct HttpTraceInfo {
    pub get_conn: u32,
    pub dns_lookup: u32,
    pub tcp_conn: u32,
    pub tls_handshake: u32,
    pub total: u32,
}

fn diff(value1: &AtomicU64, value2: &AtomicU64) -> u32 {
    (value1.load(Ordering::Relaxed) - value2.load(Ordering::Relaxed)) as u32
}

impl HttpTrace {
    fn reset(&self) {
        if self.done.load(Ordering::Relaxed) != 0 {
            self.start.store(now(), Ordering::Relaxed);
            self.get_from_pool.store(0, Ordering::Relaxed);
            self.got_conn.store(0, Ordering::Relaxed);
            self.dns_start.store(0, Ordering::Relaxed);
            self.dns_done.store(0, Ordering::Relaxed);
            self.conn_start.store(0, Ordering::Relaxed);
            self.conn_done.store(0, Ordering::Relaxed);
            self.tls_start.store(0, Ordering::Relaxed);
            self.tls_done.store(0, Ordering::Relaxed);
            self.handshake_start.store(0, Ordering::Relaxed);
            self.handshake_done.store(0, Ordering::Relaxed);
            self.done.store(0, Ordering::Relaxed);
        }
    }
    fn set_get_from_pool(&self) {
        self.get_from_pool.store(now(), Ordering::Relaxed);
    }
    fn set_got_conn(&self) {
        self.got_conn.store(now(), Ordering::Relaxed);
    }
    fn set_dns_start(&self) {
        self.dns_start.store(now(), Ordering::Relaxed);
    }
    fn set_dns_done(&self) {
        self.dns_done.store(now(), Ordering::Relaxed);
    }
    fn set_conn_start(&self) {
        self.conn_start.store(now(), Ordering::Relaxed);
    }
    fn set_conn_done(&self) {
        self.conn_done.store(now(), Ordering::Relaxed);
    }
    fn set_tls_start(&self) {
        self.tls_start.store(now(), Ordering::Relaxed);
    }
    fn set_tls_done(&self) {
        self.tls_done.store(now(), Ordering::Relaxed);
    }
    fn set_handshake_start(&self) {
        self.handshake_start.store(now(), Ordering::Relaxed);
    }
    fn set_handshake_done(&self) {
        self.handshake_done.store(now(), Ordering::Relaxed);
    }
    fn set_done(&self) {
        self.done.store(now(), Ordering::Relaxed);
    }
    fn get_trace_info(&self) -> HttpTraceInfo {
        if self.done.load(Ordering::Relaxed) == 0 {
            self.set_done();
        }
        HttpTraceInfo {
            get_conn: diff(&self.got_conn, &self.get_from_pool),
            dns_lookup: diff(&self.dns_done, &self.dns_start),
            tcp_conn: diff(&self.conn_done, &self.conn_start),
            tls_handshake: diff(&self.tls_done, &self.tls_start),
            total: diff(&self.done, &self.start),
        }
    }
}

pub fn new_default_http_trace() -> HttpTrace {
    let v = HttpTrace::default();
    v.start.store(now(), Ordering::Relaxed);
    v
}
pub fn get_http_trace_info() -> HttpTraceInfo {
    HTTP_TRACE.with(|v| v.get_trace_info())
}
pub fn reset_http_trace() {
    HTTP_TRACE.with(|v| v.reset());
}

tokio::task_local! {
    pub static HTTP_TRACE: HttpTrace;
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

struct JsonVisitor<'a>(&'a mut BTreeMap<String, String>);

impl<'a> tracing::field::Visit for JsonVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{:?}", value));
    }
}

// get conn from pool
// http connect start
// dns start
// dns done
// conn start
// tcp start
// tcp done
// tls start
// tls done
// http start
// first byte
// done
pub struct HTTPTraceLayer;
impl<S> Layer<S> for HTTPTraceLayer
where
    S: tracing::Subscriber,
    // Scary! But there's no need to even understand it. We just need it.
    S: for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let target = event.metadata().target();
        if !target.starts_with("hyper::") {
            return;
        }

        let mut fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut fields);
        event.record(&mut visitor);

        let message = fields.get("message");
        if message.is_none() {
            return;
        }
        // 已保证不会为空
        let message = message.unwrap();
        // println!("{}: {message}", now());
        match target {
            "hyper::client::pool" => {
                if message.starts_with("checkout waiting for idle connection") {
                    HTTP_TRACE.with(|v| {
                        v.set_get_from_pool();
                    });
                }
            }
            "hyper::client::connect::http" => {
                // HTTP 开始连接
                if message.starts_with("Http::connect;") {
                    // Http::connect; scheme=Some("https"), host=Some("httpbin.org"), port=None
                    HTTP_TRACE.with(|v| {
                        v.set_got_conn();
                        v.set_dns_start();
                    });
                } else if message.starts_with("connecting to") {
                    // 开始TCP连接
                    HTTP_TRACE.with(|v| {
                        v.set_dns_done();
                        v.set_conn_start();
                    });
                } else if message.starts_with("connected to") {
                    // TCP连接成功
                    HTTP_TRACE.with(|v| {
                        v.set_conn_done();
                        // 暂时不管是否有tls
                        v.set_tls_start();
                    });
                }
            }
            "hyper::client::connect::dns" => {
                // TODO dns由于是使用spawn_blocking，暂时只通过其它方式记录时长
            }
            "hyper::client::conn" => {
                // 开始连接
                if message.starts_with("client handshake") {
                    // http开始
                    HTTP_TRACE.with(|v| {
                        // 暂时不管是否有tls
                        v.set_tls_done();
                        v.set_handshake_start();
                    });
                }
            }
            "hyper::client::client" => {
                // 如果是https，包括tls
                if message.starts_with("handshake complete") {
                    // http 请求完成，开始发送数据
                    HTTP_TRACE.with(|v| v.set_handshake_done());
                }
            }
            // spawn，因此无法直接使用tokio local
            "hyper::proto::h1::conn" => {
                // // 接收完数据
                // if message.starts_with("incoming body completed") {
                //     // HTTP_TRACE.with(|v| v.set_content_transfer_done());
                // }
                // // 获取首字节
                // else if message.starts_with("Conn::read_head") {
                //     // HTTP_TRACE.with(|v| v.set_content_transfer_start());
                // }
            }
            _ => {
                // println!("{target}");
                // println!("{message}");
            }
        }
    }
}
