use std::comm::{Port, Chan, Select};
use std::io;
use std::io::{Acceptor, Listener, IoResult};
use std::io::net::tcp::{TcpListener, TcpStream};
use std::io::net::ip::{SocketAddr, Ipv4Addr};
use std::str;
use std::task;
use std::vec;

mod telnet;

#[deriving(Eq)]
pub enum ID {
	Unassigned,
	Unconnected(uint),
	Connected(uint)
}
impl ID {
	fn get(&self) -> uint {
		match *self {
			Unassigned     => fail!("Attempted to unwrap an unassigned ID."),
			Unconnected(x) => return x,
			Connected(x)   => return x
		}
	}
}

pub enum Command {
	ShutDownComplete,
	TelnetCommand(telnet::Command),
	PlayerString(ID, ~str)
}
pub enum Response {
	ShutDown,

	NewConnection(Connection),
	Disconnect(ID),

	BroadCast(~str),
	MultiCast(~[ID], ~str),
	UniCast(ID, ~str),

	Nothing
}

struct Connection {
	id : ID,
	port : Port<Option<~str>>,
	chan : Chan<Option<~str>>
}

impl Drop for Connection {
	fn drop(&mut self) {
		self.chan.try_send(None);
	}
}

impl Connection {
	fn new(stream : TcpStream) -> Connection
	{
		let (clientPort, chan) = Chan::new();
		let (port, clientChan) = Chan::new();
		let writeStream = stream.clone();
		let mut builder = task::task();

		builder.name("<generic_client_writer>");
		builder.spawn(proc(){clientWriteEntry(writeStream, clientPort)});

		builder = task::task();
		builder.name("<generic_client_reader>");
		builder.spawn(proc(){clientReadEntry(stream, clientChan)});

		Connection{id : Unassigned, port : port, chan : chan}
	}
}

pub fn netEntry(mut mainPort : Port<Response>, mainChan : Chan<Command>)
{
	let (mut listenPort, listenChan) = Chan::new();
	let mut connections = ~[];

	let mut builder = task::task();
	builder.name("listener");
	builder.spawn(proc(){listenEntry(listenChan)});

	loop {
		let maybeCommand = handleResponse(doSelect(&mut mainPort, &mut listenPort, &mainChan, connections), &mut connections);
		match maybeCommand {
			Some(command) => mainChan.send(command),
			None          => ()
		}
	}
}

fn clientReadEntry(mut stream : TcpStream, chan : Chan<Option<~str>>)
{
	let mut read_buffer = [0, ..128];
	let mut utf8_buffer = ~[];
	loop{
		match checkIoResult(stream.read(read_buffer), [io::EndOfFile]) {
			Some(_)   => utf8_buffer = vec::append(utf8_buffer, read_buffer),
			None => {chan.send(None); return}
		}
		let telnet_commands = telnet::parse(&mut utf8_buffer);
		for x in telnet_commands.iter() {
			match *x {
				telnet::Will(option)  => {
					println!("Will({})", option);
					if checkIoResult(telnet::send(&mut stream, telnet::Dont(option)), [io::EndOfFile]).is_none() {
						chan.send(None);
						return
					}
				}
				telnet::Wont(option)  => println!("Wont({})", option),
				telnet::Do(option)    => {
					println!("Do({})", option);
					if checkIoResult(telnet::send(&mut stream, telnet::Wont(option)), [io::EndOfFile]).is_none() {
						chan.send(None);
						return
					}
				}
				telnet::Dont(option)  => println!("Dont({})", option),
				telnet::Other(option) => println!("Other({})", option),
				telnet::Malformed     => {println!("Malformed telnet command."); chan.send(None); return}
			}
		}

		let maybeStr = str::from_utf8(utf8_buffer);
		if maybeStr.is_some() {
			chan.send(Some(maybeStr.unwrap().to_owned()));
		}
	}
}

fn clientWriteEntry(mut stream : TcpStream, port : Port<Option<~str>>)
{
	loop{
		let message = port.recv();
		match message {
			Some(string) => {
				let bytes = string.into_bytes();
				if checkIoResult(stream.write(bytes), [io::EndOfFile]).is_none() {
					return;
				}
				if checkIoResult(telnet::send(&mut stream, telnet::Other(telnet::GA as u8)), [io::EndOfFile]).is_none() {
					return;
				}
			}
			None => return
		}
	}
}

fn listenEntry(chan : Chan<TcpStream>)
{
	let mut acceptor = TcpListener::bind(SocketAddr{ip : Ipv4Addr(0, 0, 0, 0), port : 6666}).unwrap().listen().unwrap();
	loop {
		let newConnection = acceptor.accept();
		match newConnection {
			Ok(TcpStream) => chan.send(TcpStream),
			Err(error)    => fail!("Failure on accept() call: {}", error)
		}
	}
}

fn doSelect(mainPort : &mut Port<Response>, listenPort : &mut Port<TcpStream>,
	mainChan : &Chan<Command>, connections : &mut [Option<Connection>])
	-> Response
{
	let sel = Select::new();
	let mut mainHandle = sel.add(mainPort);
	let mut listenHandle = sel.add(listenPort);
	let mut handles = ~[];
	for x in connections.mut_iter() {
		match *x {
			Some(ref mut connection) => handles.push((connection.id, sel.add(&mut connection.port))),
			None                     => ()
		}
	}
	let ret = sel.wait();

	if ret == mainHandle.id {
		return mainHandle.recv();
	}
	else if ret == listenHandle.id {
		let stream = listenHandle.recv();
		return NewConnection(Connection::new(stream))
	}
	else {
		for &(playerID, ref mut handle) in handles.mut_iter() {
			if ret == handle.id {
				match handle.recv() {
					Some(string) => {mainChan.send(PlayerString(playerID, string)); return Nothing}
					None => return Disconnect(playerID)
				}
			}
		}
	}
	unreachable!()
}

fn handleResponse(reponse : Response, connections : &mut ~[Option<Connection>]) -> Option<Command>
{
	match reponse {
		ShutDown => {
			for x in connections.mut_iter() {
				*x = None
			}
			return Some(ShutDownComplete);
		}

		NewConnection(mut connection) => {
			let mut locationFound = false;
			for (num, x) in connections.iter().enumerate() {
				if !x.is_some() {
					connection.id = Unconnected(num);
					locationFound = true;
					break;
				}
			}
			if locationFound {
				let location = connection.id.get();
				connections[location] = Some(connection);
			} else {
				connections.push(Some(connection));
			}
		}
		Disconnect(id) => {
			for x in connections.mut_iter() {
				if x.as_ref().map_or(false, |c| c.id == id) {
					*x = None
				}
			}
		}

		BroadCast(string) => {
			for x in connections.iter().flat_map(|o| o.iter()) {
				x.chan.send(Some(string.clone())); 
			}
		}
		MultiCast(ids, string) => {
			for id in ids.iter() {
				for x in connections.iter().flat_map(|o| o.iter()) {
					if x.id == *id {
						x.chan.send(Some(string.clone()));
					}
				}
			}
		}
		UniCast(id, string) => {
			for x in connections.iter().flat_map(|o| o.iter()) {
				if x.id == id {
					x.chan.send(Some(string.clone()))
				}
			}
		}

		Nothing => ()		
	}
	return None
}

fn checkIoResult<T>(result : IoResult<T>, kinds : &[io::IoErrorKind]) -> Option<T> {
	match result {
		Ok(x) => Some(x),
		Err(what) => {
			for x in kinds.iter() {
				if what.kind == *x {
					return None;
				}
			}
			fail!("IoResult resulted in a failure with with {}", what)
		}
	}
}
