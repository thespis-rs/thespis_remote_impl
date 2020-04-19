use crate :: { import::*, *, peer::request_error::RequestError };



/// Register services to be relayed to other backend providers. The difference with the `service_map` macro, which is
/// used for local handlers is that handlers here don't have to implement `Handler<M>` for the actual message type.
/// They only have to implement `Handler<WireFormat>` (for sends) and `Handler<peer::Call>` for calls.
//
pub struct RelayMap
{
	// I decided not to take a static reference to ServiceID, because it seems kind or limiting how people
	// can store them. In this case, the process does not need to compile in the actual handlers.
	// ServiceID is just 16 bytes of data.
	//
	handler : Mutex<ServiceHandler> ,
	services: Vec<ServiceID>        ,
}


impl RelayMap
{
	/// Create a RelayMap.
	//
	pub fn new( handler: ServiceHandler, services: Vec<ServiceID> ) -> Self
	{
		Self { handler: Mutex::new( handler ), services }
	}


	// async fn handle_err( mut peer: Addr<Peer>, err: ThesRemoteErr )
	// {
	// 	if peer.send( RequestError::from( err.clone() ) ).await.is_err()
	// 	{
	// 		error!
	// 		(
	// 			"Peer ({}, {:?}): Processing incoming call: peer to client is closed, but processing request errored on: {}.",
	// 			peer.id(),
	// 			peer.name(),
	// 			&err
	// 		);
	// 	}
	// }
}



impl ServiceMap for RelayMap
{
	/// Send a message to a handler. This should take care of deserialization.
	//
	fn send_service( &self, msg: WireFormat )

		-> Result< Pin<Box< dyn Future< Output=Result<(), ThesRemoteErr> > + Send >>, ThesRemoteErr >
	{
		trace!( "RelayMap: Incoming Send for relayed actor." );

		let sid = msg.service();

		// This sid should be in our map.
		//
		match &*self.handler.lock()
		{
			ServiceHandler::Address( a ) =>
			{
				let mut a = a.clone_box();

				let task = async move
				{
					a.send( msg ).await.map_err( |_| ThesRemoteErr::HandlerDead{ ctx: Default::default() } )
				};

				Ok( task.boxed() )
			}


			ServiceHandler::Closure( c ) =>
			{
				let mut a = c(&sid);

				let task = async move
				{
					a.send( msg ).await.map_err( |_| ThesRemoteErr::HandlerDead{ ctx: Default::default() } )
				};

				Ok( task.boxed() )			}
		}
	}


	/// Call a Service.
	/// This should take care of deserialization. The return address is the address of the peer
	/// to which the serialized answer shall be send.
	//
	fn call_service( &self, frame: WireFormat, peer: Addr<Peer> )

		-> Result< Pin<Box< dyn Future< Output=Result<(), ThesRemoteErr> > + Send >>, ThesRemoteErr >
	{
		trace!( "RelayMap: Incoming Call for relayed actor." );

		let sid = frame.service();

		match &*self.handler.lock()
		{
			ServiceHandler::Address( a ) => Ok( make_call( a.clone_box(), frame, peer ).boxed() ),
			ServiceHandler::Closure( c ) => Ok( make_call( c(&sid)      , frame, peer ).boxed() ),
		}
	}

	// We need to make a Vec here because the hashmap.keys() doesn't have a static lifetime.
	//
	fn services( &self ) -> Vec<ServiceID>
	{
		self.services.clone()
	}
}


#[ allow(clippy::needless_return) ]
//
async fn make_call<T>( mut relay: Box<T>, frame: WireFormat, mut peer: Addr<Peer> )

	-> Result<(), ThesRemoteErr >

	where T: Address<Call, Error=ThesErr> + ?Sized

{
	let sid = frame.service();
	let cid = frame.conn_id();

	let called = match relay.call( Call::new( frame ) ).await
	{
		// Peer for relay still online.
		//
		Ok(c) => c,

		// For now can only be mailbox closed.
		//
		Err(_) =>
		{
			let ctx = Peer::err_ctx( &peer, sid, cid, "Process incoming Call to relay".to_string() );

			return Err( ThesRemoteErr::RelayGone{ ctx, relay_id: relay.id(), relay_name: relay.name() } );
		}
	};



	match called
	{
		// Sending out over the sink (connection) didn't fail
		//
		Ok( receiver ) =>	{ match receiver.await
		{
			// The channel was not dropped before resolving, so the relayed connection didn't close
			// until we got a response.
			//
			Ok( result ) =>
			{
				match result
				{
					// The remote managed to process the message and send a result back
					// (no connection errors like deserialization failures etc)
					//
					Ok( resp ) =>
					{
						trace!( "Peer {}: Got response from relayed call, sending out.", &peer.id() );

						// This can fail if we are no longer connected, in which case there isn't much to do.
						//
						if peer.send( resp ).await.is_err()
						{
							error!( "Peer {}: processing incoming call for relay: peer to client is closed before we finished sending a response to the request.", &peer.id() );
						}

						return Ok(())
					},

					// The relayed remote had errors while processing the request, such as deserialization.
					// Our own peer might send a Timeout error back as well.
					//
					Err(e) =>
					{
						let wire_format = Peer::prep_error( cid, &e );

						// Just forward the error to the client.
						//
						if peer.send( wire_format ).await.is_err()
						{
							error!( "Peer {}: processing incoming call for relay: peer to client is closed before we finished sending a response to the request.", &peer.id() );
						}

						return Ok(())
					},
				}
			},


			// This can only happen if the sender got dropped. Eg, if the remote relay goes down
			// Inform peer that their call failed because we lost connection to the relay after
			// it was sent out.
			//
			Err(_) =>
			{
				let ctx = Peer::err_ctx( &peer, sid, cid, "Process incoming Call to relay".to_string() );

				let err = ThesRemoteErr::RelayGone{ ctx, relay_id: relay.id(), relay_name: relay.name() };
				let err = RequestError::from( err );


				if peer.send( err ).await.is_err()
				{
					error!( "Peer {}: processing incoming call for relay: peer to client is closed before we finished sending a response to a request.", &peer.id() );
				}

				return Ok(())
			}
		}},

		// Sending out call to relayed failed. This normally only happens if the connection
		// was closed, or a network transport malfunctioned.
		//
		Err(_) =>
		{
			let ctx = Peer::err_ctx( &peer, sid, cid, "Process incoming Call to relay".to_string() );

			let err = ThesRemoteErr::RelayGone{ ctx, relay_id: relay.id(), relay_name: relay.name() };
			let err = RequestError::from( err );

			if peer.send( err ).await.is_err()
			{
				error!( "Peer {}: processing incoming call for relay: peer to client is closed before we finished sending a response to a request.", &peer.id() );
			}

			return Ok(())
		}
	}
}



/// Will print something like:
///
/// ```ignore
/// "RelayMap, handler: ServiceHandler: Address: id: {}, name: <name of handler>, services:
/// {
///    sid: 0xbcc09d3812378e171ad366d75f687757
///    sid: 0xbcc09d3812378e17e1a1e89b512c025a
/// }"
/// ```
//
impl fmt::Debug for RelayMap
{
	fn fmt( &self, f: &mut fmt::Formatter<'_> ) -> fmt::Result
	{
		write!( f, "RelayMap, handler: {}, services:\n{{\n", &*self.handler.lock() )?;

		for sid in &self.services
		{
			writeln!( f, "\tsid: 0x{:02x}", sid )?;
		}

		write!( f, "}}" )
	}
}
