
use
{
	crate     :: { import::*, PeerErr, wire_format::*        } ,
	byteorder :: { ReadBytesExt, WriteBytesExt, LittleEndian } ,
	std       :: { io::{ Seek, Write as IoWrite }            } ,
};


mod encoder;
mod decoder;
mod decoder_noheap;

pub use encoder::*;
pub use decoder::*;
pub use decoder_noheap::*;

const LEN_LEN: usize = 8; // u64
const LEN_SID: usize = 8; // u64
const LEN_CID: usize = 8; // u64


const IDX_LEN: usize = 0;
const IDX_SID: usize = LEN_LEN;
const IDX_CID: usize = IDX_SID + LEN_SID;
const IDX_MSG: usize = IDX_CID + LEN_CID;

const LEN_HEADER: usize = IDX_MSG;



/// A multi service message.
///
/// The message format is as follows:
///
/// The format is little endian.
///
/// length  : the length in bytes of the payload
/// sid     : user chosen sid for the service
/// connID  : in case of a call, which requires a response, a unique random number
///           in case of a send, which does not require response, zero
/// message : the request message serialized with the specified codec
///
/// ```text
/// u64 length + payload ------------------------------------------|
///              8 bytes sid | 8 bytes connID | serialized message |
///              u64 LE      | u64 LE         | variable           |
/// ----------------------------------------------------------------
/// ```
///
/// As soon as a codec determines from the length field that the entire message is read,
/// they can create a Multiservice from the bytes. In general creating a Multiservice
/// object should not perform a copy of the serialized message. It just provides a window
/// to look into the multiservice message to figure out if:
/// - the service sid is meant to be delivered to an actor on the current process or is to be relayed.
/// - if unknown, if there is a non-zero connID, in which case an error is returned to the sender
/// - if it is meant for the current process, deserialize it with `encoding` and send it to the correct actor.
///
/// The algorithm for receiving messages should interprete the message like this:
///
/// ```text
/// - msg for a local actor -> service sid is in our table
///   - send                -> no connID
///   - call                -> connID
///
/// - a send/call for an actor we don't know -> sid unknown: respond with error
///
/// - a response to a call    -> valid connID
///   - when the call was made, we gave a onshot-channel receiver to the caller,
///     look it up in our open connections table and put the response in there.
///
/// - an error message (eg. deserialization failed on the remote) -> service sid null.
///
///
///   -> log the error, we currently don't have a way to pass an error to the caller.
///      We should provide a mechanism for the application to handle the errors.
///      The deserialized message shoudl be a string error.
///
/// Possible errors:
/// - Destination unknown (since an address is like a capability,
///   we don't distinguish between not permitted and unknown)
/// - Fail to deserialize message
//
#[ derive( Debug, Clone, PartialEq, Eq ) ]
//
pub struct ThesWF
{
	data: io::Cursor< Vec<u8> >,
}



impl Message for ThesWF
{
	type Return = Result<(), PeerErr>;
}


impl ThesWF
{
	/// Get direct access to the buffer.
	//
	fn as_buf( &self ) -> &[u8]
	{
		self.data.get_ref()
	}

	fn set_len( &mut self, len: u64 ) -> &mut Self
	{
		self.data.get_mut()[ IDX_LEN..IDX_LEN+LEN_LEN ].as_mut().write_u64::<LittleEndian>( len ).unwrap();
		self
	}
}


// All the methods here can panic. We should make sure that bytes is always big enough,
// because bytes.slice panics if it's to small. Same for bytes.put.
//
impl WireFormat for ThesWF
{
	/// The service id of this message. When coming in over the wire, this identifies
	/// which service you are calling. A ServiceID should be unique for a given service.
	/// The reference implementation combines a unique type id with a namespace so that
	/// several processes can accept the same type of service under a unique name each.
	//
	fn sid( &self ) -> ServiceID
	{
		// TODO: is this the most efficient way?
		//
		self.data.get_ref()[ IDX_SID..IDX_SID+LEN_SID ].as_ref().read_u64::<LittleEndian>().unwrap().into()
	}


	fn set_sid( &mut self, sid: ServiceID ) -> &mut Self
	{
		self.data.get_mut()[ IDX_SID..IDX_SID+LEN_SID ].as_mut().write_u64::<LittleEndian>( sid.into() ).unwrap();
		self
	}



	/// The connection id. This is used to match responses to outgoing calls.
	//
	fn cid( &self ) -> ConnID
	{
		self.data.get_ref()[ IDX_CID..IDX_CID+LEN_CID ].as_ref().read_u64::<LittleEndian>().unwrap().into()
	}


	fn set_cid( &mut self, cid: ConnID ) -> &mut Self
	{
		self.data.get_mut()[ IDX_CID..IDX_CID+LEN_CID ].as_mut().write_u64::<LittleEndian>( cid.into() ).unwrap();
		self
	}


	/// The serialized payload message.
	//
	fn msg( &self ) -> &[u8]
	{
		&self.data.get_ref()[ IDX_MSG.. ]
	}

	/// The total length of the ThesWF in bytes (header+payload)
	//
	fn len( &self ) -> u64
	{
		self.data.get_ref()[ IDX_LEN..IDX_LEN+LEN_LEN ].as_ref().read_u64::<LittleEndian>().unwrap()
	}

	/// Make sure there is enough room for the serialized payload to avoid frequent re-allocation.
	//
	fn with_capacity( size: usize ) -> Self
	{
		trace!( "creating wf with capacity: {}", size );

		let mut wf = Self
		{
			data: io::Cursor::new( Vec::with_capacity( size + LEN_HEADER ) )
		};

		wf.data.write( &[0u8; LEN_HEADER] ).unwrap();
		wf.set_len( LEN_HEADER as u64 );

		wf
	}
}



impl io::Write for ThesWF
{
	fn write( &mut self, buf: &[u8] ) -> io::Result<usize>
	{
		self.data.seek( io::SeekFrom::End(0) )?;

		self.data.write( buf ).map(|written|
		{
			trace!( "writing wf with mesg length: {}", self.len() + written as u64 - LEN_HEADER as u64 );

			self.set_len( self.len() + written as u64 );
			written
		})
	}

	fn flush( &mut self ) -> io::Result<()>
	{
		Ok(())
	}
}



impl Default for ThesWF
{
	/// Will create a default ThesWF with an internal buffer with capacity of LEN_HEADER *2,
	/// length set to LEN_HEADER, and sid and cid are zeroed.
	//
	fn default() -> Self
	{
		let mut wf = Self
		{
			data: io::Cursor::new( Vec::with_capacity( LEN_HEADER *2 ) )
		};

		wf.write( &[0u8; LEN_HEADER] ).unwrap();

		wf
	}
}



impl TryFrom< Vec<u8> > for ThesWF
{
	type Error = WireErr;

	fn try_from( data: Vec<u8> ) -> Result< Self, WireErr >
	{
		// at least verify we have enough bytes
		// We allow an empty message. In principle I suppose a zero sized type could be
		// serialized to nothing by serde. Haven't checked though.
		//
		if data.len() < LEN_HEADER
		{
			return Err( WireErr::Deserialize{ context: "ThesWF: not enough bytes even for the header.".to_string() } );
		}

		Ok( Self { data: io::Cursor::new(data) } )
	}
}



#[ cfg(test) ]
//
mod tests
{
	// Tests:
	//
	// - creation:
	//   - default
	//   - with_capacity
	// - set_len/len equality and check the actual data
	// - set_sid/sid equality and check the actual data
	// - set_cid/cid equality and check the actual data
	//
	use super::{ *, assert_eq };
	use crate::{ wire_format::TestSuite };
	use futures::io::{ WriteHalf, ReadHalf };


	#[test]
	//
	fn default_impl()
	{
		let wf = ThesWF::default();

		assert_eq!( LEN_HEADER * 2   , wf.data.get_ref().capacity() );
		assert_eq!( LEN_HEADER as u64, wf.len()                     );
		assert_eq!( wf.len() as usize, wf.data.get_ref().len()      );

		assert!( wf.sid().is_null() );
		assert!( wf.cid().is_null() );

		assert_eq!( 0, wf.msg().len() );
	}


	#[test]
	//
	fn with_capacity()
	{
		let wf = ThesWF::with_capacity( 5 );

		assert_eq!( LEN_HEADER + 5   , wf.data.get_ref().capacity() );
		assert_eq!( LEN_HEADER as u64, wf.len()                     );
		assert_eq!( wf.len() as usize, wf.data.get_ref().len()      );

		assert!( wf.sid().is_null() );
		assert!( wf.cid().is_null() );

		assert_eq!( 0, wf.msg().len() );
	}


	#[test]
	//
	fn set_len()
	{
		let mut wf = ThesWF::default();

		wf.set_len( 33 );
		assert_eq!( wf.len(), 33 );
	}


	#[test]
	//
	fn set_sid()
	{
		let mut wf = ThesWF::default();
		let sid = ServiceID::from_seed( &[ 1, 2, 3 ] );

		wf.set_sid( sid );
		assert_eq!( wf.sid(), sid );
	}


	#[test]
	//
	fn set_cid()
	{
		let mut wf = ThesWF::default();
		let cid = ConnID::random();

		wf.set_cid( cid );
		assert_eq!( wf.cid(), cid );
	}


	fn frame( socket: Box<dyn MockConnection>, max_size: usize ) -> (Encoder<WriteHalf<Box<dyn MockConnection>>>, Decoder<ReadHalf<Box<dyn MockConnection>>>)
	{
		let (reader, writer) = socket.split();

		let stream = Decoder::new( reader, max_size );
		let sink   = Encoder::new( writer, max_size );

		(sink, stream)
	}


	#[async_std::test]
	//
	async fn decoder_encoder_heap()
	{
		let test_suite = TestSuite::new( frame );

		test_suite.run().await;
	}


	fn frame_noheap( socket: Box<dyn MockConnection>, max_size: usize ) -> (Encoder<WriteHalf<Box<dyn MockConnection>>>, DecoderNoHeap<ReadHalf<Box<dyn MockConnection>>>)
	{
		let (reader, writer) = socket.split();

		let stream = DecoderNoHeap::new( reader, max_size );
		let sink   = Encoder::new( writer, max_size );

		(sink, stream)
	}


	#[async_std::test]
	//
	async fn decoder_encoder_noheap()
	{
		let test_suite = TestSuite::new( frame_noheap );

		test_suite.run().await;
	}
}
