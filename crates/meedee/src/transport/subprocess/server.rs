//! Worker service, shared by the Rust binary and Python's interpreter bootstrap.
use super::*;
use std::io;

struct Server {
    input: Option<super::super::Input>,
    output: Option<super::super::Output>,
    input_connection: Option<super::super::InputConnection>,
    output_connection: Option<super::super::OutputConnection>,
    ports: Vec<super::super::Port>,
    delivery: Option<Arc<queue::Delivery>>,
    diagnostics: Arc<Mutex<Option<super::super::InputDiagnostics>>>,
    sender: Arc<Mutex<Box<dyn Write + Send>>>,
    limit: usize,
    generation: u64,
}
impl Server {
    fn dropped(&self) -> u64 {
        self.diagnostics
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map_or(0, super::super::InputDiagnostics::dropped_packets)
            .saturating_add(self.delivery.as_ref().map_or(0, |d| d.dropped()))
    }
    fn close(&mut self) -> Result<()> {
        if let Some(delivery) = &self.delivery {
            delivery.retire();
        }
        if let Some(connection) = self.input_connection.take() {
            self.input = Some(connection.close()?);
        }
        if let Some(connection) = self.output_connection.take() {
            self.output = Some(connection.close()?);
        }
        if let Some(delivery) = &self.delivery {
            delivery.drain();
        }
        Ok(())
    }
    fn perform(&mut self, operation: &str, args: Value) -> Result<Value> {
        match operation {
            "init" => {
                let [input, backend, timestamps, limit] = values(args)?;
                let backend = match i32::try_from(number(backend)?).map_err(worker_error)? {
                    1 => Backend::CoreMidi,
                    2 => Backend::AlsaSeq,
                    3 => Backend::AlsaRaw,
                    5 => Backend::WinMm,
                    0x1000 => Backend::AlsaRawUmp,
                    0x1001 => Backend::AlsaSeqUmp,
                    0x1002 => Backend::CoreMidiUmp,
                    0x1003 => Backend::WindowsMidiServices,
                    _ => return Err(bad()),
                };
                self.limit = usize::try_from(unsigned(limit)?).map_err(worker_error)?;
                if boolean(input)? {
                    self.input = Some(super::super::Input::new(super::super::InputOptions {
                        protocol: backend.protocol(),
                        backend: Some(backend),
                        timestamps: if boolean(timestamps)? {
                            TimestampMode::Absolute
                        } else {
                            TimestampMode::Off
                        },
                        receive_byte_limit: self.limit,
                    })?);
                } else {
                    self.output = Some(super::super::Output::new(super::super::OutputOptions {
                        protocol: backend.protocol(),
                        backend: Some(backend),
                    })?);
                }
                Ok(Value::Null)
            }
            "ports" => {
                self.ports = if let Some(input) = &self.input {
                    input.ports()?
                } else if let Some(output) = &self.output {
                    output.ports()?
                } else {
                    return Err(Error::Busy);
                };
                Ok(Value::List(
                    self.ports
                        .iter()
                        .map(|p| Value::Tuple(vec![text(p.name()), integer(p.backend_id())]))
                        .collect(),
                ))
            }
            "open" => {
                let [index, name, generation] = values(args)?;
                let name = string(name)?;
                let port = if index == Value::Null {
                    None
                } else {
                    Some(
                        self.ports
                            .get(usize::try_from(unsigned(index)?).map_err(worker_error)?)
                            .ok_or_else(|| {
                                Error::Invalid("port is not in this discovery snapshot".into())
                            })?
                            .clone(),
                    )
                };
                self.generation = unsigned(generation)?;
                self.delivery = None;
                *self.diagnostics.lock().unwrap_or_else(|p| p.into_inner()) = None;
                if let Some(input) = self.input.take() {
                    let delivery = queue::Delivery::child(
                        Arc::clone(&self.sender),
                        self.limit,
                        self.generation,
                        Arc::clone(&self.diagnostics),
                    )?;
                    let producer = Arc::clone(&delivery);
                    self.delivery = Some(delivery);
                    let callback = move |p: super::super::ReceivedPacket| {
                        producer.push(p.sequence, p.timestamp_ns, p.packet)
                    };
                    let connection = if let Some(port) = port {
                        input.connect(&port, callback)?
                    } else {
                        input.create_virtual(&name, callback)?
                    };
                    *self.diagnostics.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some(connection.diagnostics());
                    self.input_connection = Some(connection);
                } else if let Some(output) = self.output.take() {
                    self.output_connection = Some(if let Some(port) = port {
                        output.connect(&port)?
                    } else {
                        output.create_virtual(&name)?
                    });
                } else {
                    return Err(Error::Busy);
                }
                Ok(Value::Null)
            }
            "send" | "send_command" => {
                let [packets] = values(args)?;
                let packets = tuple(packets)?
                    .into_iter()
                    .map(packet)
                    .collect::<Result<Vec<_>>>()?;
                let output = self.output_connection.as_ref().ok_or(Error::Closed)?;
                for packet in &packets {
                    super::super::validate_packet(output.protocol, packet)?;
                }
                if operation == "send" {
                    if packets.len() != 1 {
                        return Err(bad());
                    }
                    output.send(&packets[0])?;
                } else {
                    output
                        .worker
                        .call(move |native| native.send_packets(&packets))?;
                }
                Ok(Value::Null)
            }
            "close" | "shutdown" => {
                self.close()?;
                Ok(Value::Null)
            }
            _ => Err(Error::Invalid("unknown worker operation".into())),
        }
    }
}

/// Run the worker on standard input/output. Intended only for a dedicated child.
/// EOF exits the worker process even when a native operation is blocked.
pub fn serve() -> Result<()> {
    let mut sender: Box<dyn Write + Send> = Box::new(io::BufWriter::new(io::stdout()));
    sender
        .write_all(wire::MAGIC)
        .and_then(|_| sender.flush())
        .map_err(worker_error)?;
    let sender = Arc::new(Mutex::new(sender));
    let (requests, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("meedee worker input".into())
        .spawn(move || {
            let mut stream = io::BufReader::new(io::stdin());
            let mut magic = [0; 8];
            if stream.read_exact(&mut magic).is_err() || &magic != wire::MAGIC {
                std::process::exit(2);
            }
            loop {
                match wire::read(&mut stream) {
                    Ok(value) => {
                        if requests.send(value).is_err() {
                            return;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                        std::process::exit(0)
                    }
                    Err(_) => std::process::exit(2),
                }
            }
        })
        .map_err(worker_error)?;
    let mut server = Server {
        input: None,
        output: None,
        input_connection: None,
        output_connection: None,
        ports: Vec::new(),
        delivery: None,
        diagnostics: Arc::new(Mutex::new(None)),
        sender: Arc::clone(&sender),
        limit: 8_388_608,
        generation: 0,
    };
    for request in receiver {
        let [operation, args] = values(request)?;
        let operation = string(operation)?;
        let result = server.perform(&operation, args);
        let fatal = match &result {
            Err(error) => {
                let nonfatal = matches!(
                    error,
                    Error::Capability(_) | Error::Invalid(_) | Error::Busy | Error::CallbackThread
                ) || operation == "ports"
                    && cfg!(target_os = "macos")
                    && matches!(error, Error::Native(_));
                operation == "open" || !nonfatal
            }
            _ => false,
        };
        let message = match result {
            Ok(value) => Value::Tuple(vec![
                text(if operation == "shutdown" {
                    "shutdown"
                } else {
                    "ok"
                }),
                Value::Tuple(vec![value, integer(server.dropped())]),
            ]),
            Err(error) => Value::Tuple(vec![
                text("error"),
                Value::Tuple(vec![
                    error_value(&error),
                    Value::Bool(fatal),
                    integer(server.dropped()),
                ]),
            ]),
        };
        let mut stream = sender.lock().unwrap_or_else(|p| p.into_inner());
        wire::write(&mut *stream, &message)
            .and_then(|_| stream.flush())
            .map_err(worker_error)?;
        drop(stream);
        if fatal || operation == "shutdown" {
            return Ok(());
        }
    }
    Ok(())
}
