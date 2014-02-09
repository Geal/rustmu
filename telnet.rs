use std::io::IoResult;
use std::io::net::tcp::TcpStream;
use std::num::FromPrimitive;

#[deriving(Show)]
pub enum Command {
	Will(u8),
	Wont(u8),
	Do(u8),
	Dont(u8),
	Other(u8),
	Malformed
}

#[deriving(FromPrimitive, Show)]
pub enum OtherCommand {
	SE  = 240,
	NOP = 241,
	DM  = 242,
	BRK = 243,
	IP  = 244,
	AO  = 245,
	AYT = 246,
	EC  = 247,
	EL  = 248,
	GA  = 249,
	SB  = 250,

	WILL = 251,
	WONT = 252,
	DO   = 253,
	DONT = 254,

	IAC  = 255
}

#[deriving(Eq)]
enum ParsingState {
	Normal,
	InCommand,
	InNegotiate,
	InSubNegotiate
}

pub fn parse(buffer : &mut ~[u8]) -> ~[Command]
{
	let mut commands = ~[];
	let mut currently_building = Malformed;
	let mut state = Normal;

	for x in buffer.mut_iter(){
		if state == InCommand {
			match *x {
				240..250 => {state = Normal; commands.push(Other(FromPrimitive::from_u8(*x).unwrap())); *x = 0;}
				251      => {state = InNegotiate; currently_building = Will(0); *x = 0;}
				252      => {state = InNegotiate; currently_building = Wont(0); *x = 0;}
				253      => {state = InNegotiate; currently_building = Do(0);   *x = 0;}
				254      => {state = InNegotiate; currently_building = Dont(0); *x = 0;}
				255      => state = Normal,
				_        => return ~[Malformed]
			}
		} else if state == InNegotiate {
			state = Normal;
			match currently_building {
				Will(_) => {commands.push(Will(*x)); *x = 0},
				Wont(_) => {commands.push(Wont(*x)); *x = 0},
				Do(_)   => {commands.push(Do(*x)); *x = 0},
				Dont(_) => {commands.push(Dont(*x)); *x = 0},
				_       => fail!("Telnet parsing error: Attempted to negotiate something that isn't Do/Dont/Will/Wont.")	
			}
		} else {
			if *x == IAC as u8 {
				*x = 0;
				state = InCommand;
			}
		}
	}
	commands
}

pub fn send(stream : &mut TcpStream, command : Command) -> IoResult<()> {
	match command {
		Will(option)  => stream.write([IAC as u8, WILL as u8, option]),
		Wont(option)  => stream.write([IAC as u8, WONT as u8, option]),
		Do(option)    => stream.write([IAC as u8, DO as u8, option]),
		Dont(option)  => stream.write([IAC as u8, DONT as u8, option]),
		Other(option) => stream.write([IAC as u8, option]),
		Malformed     => fail!("Attempted to send a malformed telnet command.")
	}
}
