#![ cfg( feature = "tokio_codec" ) ]

mod common;

use
{
	common::{ remotes, Add, Sum, Show, import::{ *, assert_eq } } ,
	futures_ringbuf::TokioEndpoint ,
	tokio_util::codec::{ Framed } ,
	tokio::io::{ AsyncWriteExt } ,
};


// Tests:
//
// - ✔ basic remote funcionality: intertwined sends and calls.
// - ✔ correct async behavior: verify that a peer can continue to send/receive while waiting for the response to a call.
// - ✔ call a remote service after the connection has closed: verify peer event and error kind.

pub fn peer_listen( socket: TokioEndpoint, sm: Arc<impl ServiceMap + Send + Sync + 'static>, exec: impl Spawn + Clone + Send + Sync + 'static, name: &'static str ) -> (Addr<Peer>, Events<PeerEvent>)
{
	// Create mailbox for peer
	//
	let mb_peer  : Inbox<Peer> = Inbox::new( Some( name.into() ) );
	let peer_addr              = Addr ::new( mb_peer.sender() );

	// create peer with stream/sink
	//
	let mut peer = Peer::from_tokio_async_read( peer_addr.clone(), socket, 1024, exec.clone() ).expect( "spawn peer" );

	let peer_evts = peer.observe( ObserveConfig::default() ).expect( "pharos not closed" );

	// register service map with peer
	//
	peer.register_services( sm );

	exec.spawn( mb_peer.start_fut(peer) ).expect( "start mailbox of Peer" );

	(peer_addr, peer_evts)
}


pub async fn peer_connect( socket: TokioEndpoint, exec: impl Spawn + Clone + Send + Sync + 'static, name: &'static str ) -> (Addr<Peer>, Events<PeerEvent>)
{
	// Create mailbox for peer
	//
	let mb  : Inbox<Peer> = Inbox::new( Some( name.into() ) );
	let addr              = Addr ::new( mb.sender() );

	// create peer with stream/sink + service map
	//
	let mut peer = Peer::from_tokio_async_read( addr.clone(), socket, 1024, exec.clone() ).expect( "spawn peer" );

	let evts = peer.observe( ObserveConfig::default() ).expect( "pharos not closed" );

	debug!( "start mailbox for [{}] in peer_connect", name );

	exec.spawn( mb.start_fut(peer) ).expect( "start mailbox of Peer" );

	(addr, evts)
}



// Test basic remote funcionality. Test intertwined sends and calls.
//
#[test]
//
fn remote()
{
	// flexi_logger::Logger::with_str( "trace" ).start().unwrap();

	let (server, client) = TokioEndpoint::pair( 64, 64 );

	let exec = ThreadPool::new().expect( "create threadpool" );
	let ex1  = exec.clone();
	let ex2  = exec.clone();


	let peera = async move
	{
		// Create mailbox for our handler
		//
		let addr_handler = Addr::try_from( Sum(0), &ex1 ).expect( "spawn actor mailbox" );

		// Create a service map
		//
		let mut sm = remotes::Services::new();
		// Register our handlers
		//
		sm.register_handler::<Add >( Receiver::new( addr_handler.clone_box() ) );
		sm.register_handler::<Show>( Receiver::new( addr_handler.clone_box() ) );

		// get a framed connection
		//
		let _ = peer_listen( server, Arc::new( sm ), ex1.clone(), "peera" );

		trace!( "end of peera" );
	};


	let peerb = async move
	{
		let (mut peera, _)  = peer_connect( client, ex2, "peer_b_to_peera" ).await;

		// Call the service and receive the response
		//
		let mut addr  = remotes::RemoteAddr::new( peera.clone() );

		let resp = addr.call( Add(5) ).await.expect( "Call failed" );
		assert_eq!( (), resp );

		addr.send( Add(5) ).await.expect( "Send failed" );

		let resp = addr.call( Show ).await.expect( "Call failed" );
		assert_eq!( 10, resp );

		peera.send( CloseConnection{ remote: false } ).await.expect( "close connection to peera" );
	};


	// As far as I can tell, execution order is not defined, so hmm, there is no
	// guarantee that a is listening before b tries to connect, but it seems to work for now.
	//
	block_on( join( peera, peerb ) );
}




#[ derive( Actor ) ]
//
pub struct Parallel
{
	pub sum: Box< dyn Address<Show, Error=ThesRemoteErr> >,
}


impl Handler< Show > for Parallel
{
	fn handle( &mut self, _: Show ) -> Return<u64> { Box::pin( async move
	{
		self.sum.call( Show ).await.expect( "call sum" )
	})}
}


service_map!
(
	namespace     : parallel ;
	services      : Show     ;
);




// Test correct async behavior. Verify that a peer can continue to
// send/receive while waiting for the response to a call.
//
#[test]
//
fn parallel()
{
	let (server, client) = TokioEndpoint::pair( 64, 64 );

	let exec = ThreadPool::new().expect( "create threadpool" );
	let ex1  = exec.clone();
	let ex2  = exec.clone();

	let peera = async move
	{
		let codec: ThesCodec = ThesCodec::new(1024);

		// get a framed connection
		//
		let (sink_a, stream_a) = Framed::new( server, codec ).split();

		// Create mailbox for peer
		//
		let mb_peer  : Inbox<Peer> = Inbox::new( Some( "peera".into() )  );
		let peer_addr              = Addr ::new( mb_peer.sender() );

		// create peer with stream/sink
		//
		let mut peer = Peer::new( peer_addr.clone(), stream_a, sink_a, ex1.clone() ).expect( "spawn peer" );

		// Create recipients
		//
		let addr = remotes::RemoteAddr::new( peer_addr );

		// Create mailbox for our handler
		//
		let addr_handler = Addr::try_from( Parallel{ sum: Box::new( addr ) }, &ex1 ).expect( "spawn actor mailbox" );

		// register Sum with peer as handler for Add and Show
		//
		let mut sm = parallel::Services::new();
		sm.register_handler::<Show>( Receiver::new( addr_handler.clone_box() ) );

		peer.register_services( Arc::new( sm ) );

		mb_peer.start( peer, &ex1 ).expect( "Failed to start mailbox of Peer" );
	};


	let peerb = async move
	{
		// Create mailbox for peer
		//
		let     mb_peer  : Inbox<Peer> = Inbox::new( Some( "peer_b".into() )  );
		let mut peer_addr              = Addr ::new( mb_peer.sender() );

		// create peer with stream/sink
		//
		let mut peer = Peer::from_tokio_async_read( peer_addr.clone(), client, 1024, ex2.clone() ).expect( "spawn peer" );

		// Create mailbox for our handler
		//
		let addr_handler = Addr::try_from( Sum(19), &ex2 ).expect( "spawn actor mailbox" );


		// register Sum with peer as handler for Add and Show
		//
		let mut sm = remotes::Services::new();
		sm.register_handler::<Show>( Receiver::new( addr_handler.clone_box() ) );

		peer.register_services( Arc::new( sm ) );

		mb_peer.start( peer, &ex2 ).expect( "Failed to start mailbox of Peer" );


		// Create recipients
		//
		let mut remote = parallel::RemoteAddr::new( peer_addr.clone() );

		let resp = remote.call( Show ).await.expect( "Call failed" );
		assert_eq!( 19, resp );

		// dbg!( resp );

		peer_addr.send( CloseConnection{ remote: false } ).await.expect( "close connection to peera" );
	};

	block_on( join( peera, peerb ) );
}




// Test calling a remote service after the connection has closed.
//
#[test]
//
fn call_after_close_connection()
{
	// flexi_logger::Logger::with_str( "trace" ).start().unwrap();

	let (mut server, client) = TokioEndpoint::pair( 64, 64 );

	let exec = ThreadPool::new().expect( "create threadpool" );
	let ex1  = exec.clone();

	let nodea = async move
	{
		server.shutdown().await.expect( "close connection" );
	};


	let nodeb = async move
	{
		let (peera, mut peera_evts) = peer_connect( client, ex1, "nodeb_to_node_a" ).await;

		// Call the service and receive the response
		//
		let mut addr = remotes::RemoteAddr::new( peera.clone() );

		assert_eq!( PeerEvent::ClosedByRemote, peera_evts.next().await.unwrap() );


		match addr.call( Add(5) ).await
		{
			Ok (_) => unreachable!(),
			Err(e) =>
			{
				match e
				{
					ThesRemoteErr::ConnectionClosed{..} => assert!( true )                  ,
					_                                   => panic!( "wrong error: {:?}", e ) ,
				}
			}
		}
	};


	// As far as I can tell, execution order is not defined, so hmm, there is no
	// guarantee that a is listening before b tries to connect, but it seems to work for now.
	//
	block_on( join( nodea, nodeb ) );
}
