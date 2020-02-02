#![ allow( dead_code ) ]

pub mod actors;


pub mod import
{
	pub use
	{
		async_executors     :: { LocalPool, AsyncStd, ThreadPool, JoinHandle, SpawnHandle, LocalSpawnHandle } ,
		futures_ringbuf     :: { Endpoint                                                                   } ,
		thespis             :: { *                                                                          } ,
		thespis_impl        :: { *                                                                          } ,
		thespis_remote_impl :: { *, service_map, peer                                                       } ,
		log                 :: { *                                                                          } ,
		bytes               :: { Bytes, BytesMut                                                            } ,
		pharos              :: { Observable, ObserveConfig, Events                                          } ,

		std::
		{
			net          :: SocketAddr ,
			convert      :: TryFrom    ,
			future       :: Future     ,
			pin          :: Pin        ,
			sync         :: Arc        ,
			sync::atomic :: { AtomicUsize, Ordering::Relaxed } ,
		},

		futures::
		{
			channel :: { mpsc                                                                    } ,
			io      :: { AsyncWriteExt                                                           } ,
			compat  :: { Compat01As03Sink, Stream01CompatExt, Sink01CompatExt, Future01CompatExt } ,
			stream  :: { StreamExt, SplitSink, SplitStream                                       } ,
			future  :: { FutureExt, join, join3, RemoteHandle                                    } ,
			task    :: { SpawnExt, LocalSpawnExt, Spawn                                          } ,
			executor:: { block_on                                                                } ,
		},

		pretty_assertions :: { assert_eq, assert_ne } ,
		assert_matches    :: { assert_matches       } ,

	};
}


    use import::*;
pub use actors::*;


pub fn peer_listen( socket: Endpoint, sm: Arc<impl ServiceMap + Send + Sync + 'static>, exec: Arc<dyn Spawn + Send + Sync + 'static >, name: &str )

	-> (Addr<Peer>, Events<PeerEvent>, RemoteHandle<()>)
{
	// Create mailbox for peer
	//
	let mb_peer  : Inbox<Peer> = Inbox::new( Some( name.into() ) );
	let peer_addr              = Addr ::new( mb_peer.sender() );

	// create peer with stream/sink
	//
	let mut peer = Peer::from_async_read( peer_addr.clone(), socket, 1024, exec.clone() ).expect( "spawn peer" );

	let peer_evts = peer.observe( ObserveConfig::default() ).expect( "pharos not closed" );

	// register service map with peer
	//
	peer.register_services( sm );

	let (fut, handle) = mb_peer.start_fut(peer).remote_handle();

	exec.spawn( fut ).expect( "start mailbox of Peer" );

	(peer_addr, peer_evts, handle)
}




pub fn peer_connect( socket: Endpoint, exec: Arc<dyn Spawn + Send + Sync + 'static >, name: &str ) -> (Addr<Peer>, Events<PeerEvent>)
{
	// Create mailbox for peer
	//
	let mb  : Inbox<Peer> = Inbox::new( Some( name.into() ) );
	let addr              = Addr ::new( mb.sender() );

	// create peer with stream/sink + service map
	//
	let mut peer = Peer::from_async_read( addr.clone(), socket, 1024, exec.clone() ).expect( "spawn peer" );

	let evts = peer.observe( ObserveConfig::default() ).expect( "pharos not closed" );

	debug!( "start mailbox for [{}] in peer_connect", name );

	exec.spawn( mb.start_fut(peer) ).expect( "start mailbox of Peer" );

	(addr, evts)
}


pub fn provider( name: Option<Arc<str>>, exec: Arc<dyn Spawn + Send + Sync + 'static > ) -> (Endpoint, RemoteHandle<()>)
{
	let name = name.map( |n| n.to_string() ).unwrap_or( "unnamed".to_string() );
	// Create mailbox for our handler
	//
	debug!( "start mailbox for Sum handler in provider: {}", name );
	let addr_handler = Addr::try_from( Sum(0), &exec ).expect( "spawn actor mailbox" );


	// register Sum with peer as handler for Add and Show
	//
	let sm = remotes::Services::new();

	sm.register_handler::<Add >( addr_handler.clone_box() );
	sm.register_handler::<Show>( addr_handler.clone_box() );


	// get a framed connection
	//
	let (ab, ba) = Endpoint::pair( 128, 128 );

	debug!( "start mailbox for provider" );
	let (peer_addr, _peer_evts, handle) = peer_listen( ab, Arc::new( sm ), exec, "provider" );

	drop( peer_addr );
	trace!( "End of provider" );

	(ba, handle)
}



// Helper method to create relays
//
pub async fn relay
(
	connect   : Endpoint                                 ,
	listen    : Endpoint                                 ,
	next      : Pin<Box< dyn Future<Output=()> + Send >> ,
	relay_show: bool                                     ,
	exec      : Arc<dyn Spawn + Send + Sync + 'static >  ,
)
{
	debug!( "start mailbox for relay_to_provider" );

	let (mut provider_addr, _provider_evts) = peer_connect( connect, exec.clone(), "relay_to_provider" );
	let provider_addr2                      = provider_addr.clone();
	let ex1                                 = exec.clone();

	// Relay part ---------------------

	let relay = async move
	{
		// Create mailbox for peer
		//
		let mb_peer  : Inbox<Peer> = Inbox::new( Some( "relay_to_consumer".into() ) );
		let peer_addr              = Addr ::new( mb_peer.sender()                   );

		// create peer with stream/sink + service map
		//
		let mut peer = Peer::from_async_read( peer_addr, listen, 1024, ex1 ).expect( "spawn peer" );

		let add  = <Add  as remotes::Service>::sid();
		let show = <Show as remotes::Service>::sid();

		let handler: Box<dyn Relay> = Box::new( provider_addr2 );

		let mut relayed = vec![ add.clone() ];

		if relay_show
		{
			relayed.push( show.clone() );
		}

		let rm = Arc::new( RelayMap::new( handler.into(), relayed ) );
		peer.register_services( rm );

		debug!( "start mailbox for relay_to_consumer" );
		mb_peer.start_fut( peer ).await;
		warn!( "relay async block finished" );
	};


	let (relay_fut, relay_outcome) = relay.remote_handle();
	exec.spawn( relay_fut ).expect( "failed to spawn server" );

	// we need to spawn this after this relay, otherwise this relay is not listening yet when we try to connect.
	//
	exec.spawn( next ).expect( "Spawn next" );


	// If the consumer closes the connection, close our connection to provider.
	//
	relay_outcome.await;
	warn!( "relay finished, closing connection" );

	provider_addr.send( CloseConnection{ remote: false, reason: "Program end.".to_string() } ).await.expect( "close connection to provider" );
}



// Helper method to create relays
//
pub async fn relay_closure
(
	connect   : Vec<Endpoint>                            ,
	listen    : Endpoint                                 ,
	next      : Pin<Box< dyn Future<Output=()> + Send >> ,
	relay_show: bool                                     ,
	exec      : Arc<dyn Spawn + Send + Sync + 'static >  ,
)
{
	debug!( "start mailbox for relay_to_provider" );

	let mut providers: Vec<Addr<Peer>> = Vec::new();

	for (idx, endpoint) in connect.into_iter().enumerate()
	{
		let name = format!( "relay_to_provider{}", idx );
		let (provider_addr, _provider_evts) = peer_connect( endpoint, exec.clone(), &name );

		providers.push( provider_addr );
	}

	let providers2 = providers.clone();
	let ex1        = exec.clone();

	// Relay part ---------------------

	let relay = async move
	{
		// Create mailbox for peer
		//
		let mb_peer  : Inbox<Peer> = Inbox::new( Some( "relay_to_consumer".into() ) );
		let peer_addr              = Addr ::new( mb_peer.sender()                   );

		// create peer with stream/sink + service map
		//
		let mut peer = Peer::from_async_read( peer_addr, listen, 1024, ex1 ).expect( "spawn peer" );

		let add  = <Add  as remotes::Service>::sid();
		let show = <Show as remotes::Service>::sid();


		let handler = Box::new( move |_: &ServiceID| -> Box<dyn Relay>
		{
			static IDX: AtomicUsize = AtomicUsize::new( 0 );

			let   i  = IDX.fetch_add( 1, Relaxed );
			let addr = &providers2[ i % providers2.len() ];

			Box::new( addr.clone() )

		});


		let mut relayed = vec![ add.clone() ];

		if relay_show
		{
			relayed.push( show.clone() );
		}

		let rm = Arc::new( RelayMap::new( ServiceHandler::Closure( handler ), relayed ) );

		peer.register_services( rm );

		debug!( "start mailbox for relay_to_consumer" );
		mb_peer.start_fut( peer ).await;
		warn!( "relay async block finished" );
	};


	let (relay_fut, relay_outcome) = relay.remote_handle();
	exec.spawn( relay_fut ).expect( "failed to spawn server" );

	// we need to spawn this after this relay, otherwise this relay is not listening yet when we try to connect.
	//
	exec.spawn( next ).expect( "Spawn next" );


	// If the consumer closes the connection, close our connection to provider.
	//
	relay_outcome.await;
	warn!( "relay finished, closing connection" );

	for mut addr in providers
	{
		addr.send( CloseConnection{ remote: false, reason: "Program end.".to_string() } ).await.expect( "close connection to provider" );
	}
}





service_map!
(
	namespace: remotes        ;
	services : Add, Sub, Show ;
);

