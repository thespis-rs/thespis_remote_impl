// Tests:
//
// ✔ do not exceed max concurrent requests
// ✔ do not exceed max concurrent requests over several connections
// ✔ do eventually process all requests
// ✔ back pressure does not apply to sends
// ✔ back pressure does not apply to relays
// ✔ do not deadlock
//
mod common;

use
{
	common        :: { *, import::{ * }                                        } ,
	std           :: { time::Duration, sync::atomic::{ AtomicUsize, Ordering } } ,
	futures_timer :: { Delay                                                   } ,
	serde         :: { Serialize, Deserialize                                  } ,
	crate         :: { peer::BackPressure                                      } ,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);


#[ derive(Actor) ] struct Slow;

impl Handler<Add> for Slow
{
	fn handle( &mut self, _msg: Add ) -> Return<'_, ()> { async move
	{
		log::warn!( "handle Add" );

		let _ = Delay::new( Duration::from_millis(100) ).await;

		COUNTER.fetch_add( 1, Ordering::SeqCst );

	}.boxed() }
}

#[ derive( Serialize, Deserialize, Debug ) ] pub struct Add2( pub i64 );
impl Message for Add2  { type Return = ();  }

impl Handler<Add2> for Slow
{
	fn handle( &mut self, _msg: Add2 ) -> Return<'_, ()> { async move
	{
		log::warn!( "handle Add" );

		let _ = Delay::new( Duration::from_millis(50) ).await;

		COUNTER.fetch_add( 1, Ordering::SeqCst );

	}.boxed() }
}



#[ derive(Actor) ] struct After;

impl Handler<Show> for After
{
	fn handle( &mut self, _msg: Show ) -> Return<'_, i64> { async move
	{
		log::warn!( "handle Show" );

		COUNTER.load( Ordering::SeqCst ) as i64

	}.boxed() }
}



service_map!
(
	namespace: bpsm            ;
	services : Add, Add2, Show ;
);



// We create a server with a backpressure of max 2 concurrent requests. Then we send 2 requests which have
// a 50ms delay in processing. Then we send a third request which should not run before the first one has
// finished due to the backpressure requirement. We verify that it has run by checking an AtomicUsize.
//
// When changing the backpressure to 3 below, the test should fail.
//
#[test]
//
fn backpressure_basic()
{
	// flexi_logger::Logger::with_str( "trace" ).start().unwrap();

	let (server, client) = Endpoint::pair( 64, 64 );

	let exec = Arc::new( ThreadPool::new().expect( "create threadpool" ) );
	let ex1  = exec.clone();
	let ex2  = exec.clone();
	let ex3  = exec.clone();


	let peera = async move
	{
		// Create mailbox for our handler
		//
		let slow  = Addr::try_from( Slow , &ex1 ).expect( "spawn actor mailbox" );
		let slow2 = Addr::try_from( Slow , &ex1 ).expect( "spawn actor mailbox" );
		let after = Addr::try_from( After, &ex1 ).expect( "spawn actor mailbox" );

		// Create a service map
		//
		let sm = bpsm::Services::new();

		// Register our handlers
		//
		sm.register_handler::<Add >( slow .clone_box() );
		sm.register_handler::<Add2>( slow2.clone_box() );
		sm.register_handler::<Show>( after.clone_box() );


		// Create mailbox for peer
		//
		let mb_peer  : Inbox<Peer> = Inbox::new( Some( "server".into() ) );
		let peer_addr              = Addr ::new( mb_peer.sender() );

		// create peer with stream/sink
		//
		let mut peer = Peer::from_async_read( peer_addr, server, 1024, exec.clone(), Some( Arc::new( BackPressure::new(2) ) ) ).expect( "spawn peer" );


		// register service map with peer
		//
		peer.register_services( Arc::new( sm ) );

		let (fut, handle) = mb_peer.start_fut(peer).remote_handle();

		exec.spawn( fut ).expect( "start mailbox of Peer" );
		handle.await;

		trace!( "end of peera" );
	};


	let peerb = async move
	{
		let (mut peera, _)  = peer_connect( client, ex2, "peer_b_to_peera" );

		// Call the service and receive the response
		//
		let mut addr  = bpsm::RemoteAddr::new( peera.clone() );
		let mut addr2 = addr.clone();
		let mut addr3 = addr.clone();

		let add1 = async move { addr .call( Add (1) ).await.expect( "call add1"  ) };
		let add2 = async move { addr2.call( Add2(1) ).await.expect( "call add2"  ) };
		let show = async move { addr3.call( Show    ).await.expect( "call check" ) };

		let (add1_fut, add1_handle) = add1.remote_handle();
		let (add2_fut, add2_handle) = add2.remote_handle();
		let (show_fut, show_handle) = show.remote_handle();

		ex3.spawn( add1_fut ).expect( "spawn add1"  );
		ex3.spawn( add2_fut ).expect( "spawn add2"  );

		let _ = Delay::new( Duration::from_millis(10) ).await;
		ex3.spawn( show_fut ).expect( "spawn check" );

		add1_handle.await;
		add2_handle.await;

		// Add1 should be guaranteed to have finished and updated the counter to 1 before
		// show can run due to back pressure.
		//
		assert!( show_handle.await > 0 );

		peera.send( CloseConnection{ remote: false, reason: "Program end.".to_string() } ).await.expect( "close connection to peera" );
	};


	// As far as I can tell, execution order is not defined, so hmm, there is no
	// guarantee that a is listening before b tries to connect, but it seems to work for now.
	//
	block_on( join( peera, peerb ) );
}
