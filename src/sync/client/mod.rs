pub mod session;

use super::{ChangeStream, ClientSession, Database, SessionChangeStream};
use crate::{
    bson::Document,
    change_stream::{event::ChangeStreamEvent, options::ChangeStreamOptions},
    concern::{ReadConcern, WriteConcern},
    error::Result,
    options::{
        ClientOptions,
        DatabaseOptions,
        ListDatabasesOptions,
        SelectionCriteria,
        SessionOptions,
    },
    results::DatabaseSpecification,
    runtime,
    Client as AsyncClient,
};

/// This is the main entry point for the synchronous API. A `Client` is used to connect to a MongoDB
/// cluster. By default, it will monitor the topology of the cluster, keeping track of any changes,
/// such as servers being added or removed.
///
/// `Client` is a wrapper around the asynchronous [`mongodb::Client`](../struct.Client.html), and it
/// starts up an async-std runtime internally to run that wrapped client on.
///
/// `Client` uses [`std::sync::Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html) internally,
/// so it can safely be shared across threads. For example:
///
/// ```rust
/// # use mongodb::{bson::Document, sync::Client, error::Result};
/// #
/// # fn start_workers() -> Result<()> {
/// let client = Client::with_uri_str("mongodb://example.com")?;
///
/// for i in 0..5 {
///     let client_ref = client.clone();
///
///     std::thread::spawn(move || {
///         let collection = client_ref.database("items").collection::<Document>(&format!("coll{}", i));
///
///         // Do something with the collection
///     });
/// }
/// #
/// # // Technically we should join the threads here, but for the purpose of the example, we'll just
/// # // sleep for a bit.
/// # std::thread::sleep(std::time::Duration::from_secs(3));
/// # Ok(())
/// # }
/// ```
///
/// ## TCP Keepalive
/// TCP keepalive is enabled by default with ``tcp_keepalive_time`` set to 120 seconds. The
/// driver does not set ``tcp_keepalive_intvl``. See the
/// [MongoDB Diagnostics FAQ keepalive section](https://docs.mongodb.com/manual/faq/diagnostics/#does-tcp-keepalive-time-affect-mongodb-deployments)
/// for instructions on setting these values at the system level.
#[derive(Clone, Debug)]
pub struct Client {
    async_client: AsyncClient,
}

impl From<AsyncClient> for Client {
    fn from(async_client: AsyncClient) -> Self {
        Self { async_client }
    }
}

impl Client {
    /// Creates a new `Client` connected to the cluster specified by `uri`. `uri` must be a valid
    /// MongoDB connection string.
    ///
    /// See the documentation on
    /// [`ClientOptions::parse`](../options/struct.ClientOptions.html#method.parse) for more
    /// details.
    pub fn with_uri_str(uri: impl AsRef<str>) -> Result<Self> {
        let async_client = runtime::block_on(AsyncClient::with_uri_str(uri.as_ref()))?;
        Ok(Self { async_client })
    }

    /// Creates a new `Client` connected to the cluster specified by `options`.
    pub fn with_options(options: ClientOptions) -> Result<Self> {
        let async_client = AsyncClient::with_options(options)?;
        Ok(Self { async_client })
    }

    /// Gets the default selection criteria the `Client` uses for operations..
    pub fn selection_criteria(&self) -> Option<&SelectionCriteria> {
        self.async_client.selection_criteria()
    }

    /// Gets the default read concern the `Client` uses for operations.
    pub fn read_concern(&self) -> Option<&ReadConcern> {
        self.async_client.read_concern()
    }

    /// Gets the default write concern the `Client` uses for operations.
    pub fn write_concern(&self) -> Option<&WriteConcern> {
        self.async_client.write_concern()
    }

    /// Gets a handle to a database specified by `name` in the cluster the `Client` is connected to.
    /// The `Database` options (e.g. read preference and write concern) will default to those of the
    /// `Client`.
    ///
    /// This method does not send or receive anything across the wire to the database, so it can be
    /// used repeatedly without incurring any costs from I/O.
    pub fn database(&self, name: &str) -> Database {
        Database::new(self.async_client.database(name))
    }

    /// Gets a handle to a database specified by `name` in the cluster the `Client` is connected to.
    /// Operations done with this `Database` will use the options specified by `options` by default
    /// and will otherwise default to those of the `Client`.
    ///
    /// This method does not send or receive anything across the wire to the database, so it can be
    /// used repeatedly without incurring any costs from I/O.
    pub fn database_with_options(&self, name: &str, options: DatabaseOptions) -> Database {
        Database::new(self.async_client.database_with_options(name, options))
    }

    /// Gets a handle to the default database specified in the `ClientOptions` or MongoDB connection
    /// string used to construct this `Client`.
    ///
    /// If no default database was specified, `None` will be returned.
    pub fn default_database(&self) -> Option<Database> {
        self.async_client.default_database().map(Database::new)
    }

    /// Gets information about each database present in the cluster the Client is connected to.
    pub fn list_databases(
        &self,
        filter: impl Into<Option<Document>>,
        options: impl Into<Option<ListDatabasesOptions>>,
    ) -> Result<Vec<DatabaseSpecification>> {
        runtime::block_on(
            self.async_client
                .list_databases(filter.into(), options.into()),
        )
    }

    /// Gets the names of the databases present in the cluster the Client is connected to.
    pub fn list_database_names(
        &self,
        filter: impl Into<Option<Document>>,
        options: impl Into<Option<ListDatabasesOptions>>,
    ) -> Result<Vec<String>> {
        runtime::block_on(
            self.async_client
                .list_database_names(filter.into(), options.into()),
        )
    }

    /// Starts a new `ClientSession`.
    pub fn start_session(&self, options: Option<SessionOptions>) -> Result<ClientSession> {
        runtime::block_on(self.async_client.start_session(options)).map(Into::into)
    }

    /// Starts a new [`ChangeStream`] that receives events for all changes in the cluster. The
    /// stream does not observe changes from system collections or the "config", "local" or
    /// "admin" databases. Note that this method (`watch` on a cluster) is only supported in
    /// MongoDB 4.0 or greater.
    ///
    /// See the documentation [here](https://docs.mongodb.com/manual/changeStreams/) on change
    /// streams.
    ///
    /// Change streams require either a "majority" read concern or no read
    /// concern. Anything else will cause a server error.
    ///
    /// Note that using a `$project` stage to remove any of the `_id` `operationType` or `ns` fields
    /// will cause an error. The driver requires these fields to support resumability. For
    /// more information on resumability, see the documentation for
    /// [`ChangeStream`](change_stream/struct.ChangeStream.html)
    ///
    /// If the pipeline alters the structure of the returned events, the parsed type will need to be
    /// changed via [`ChangeStream::with_type`].
    pub fn watch(
        &self,
        pipeline: impl IntoIterator<Item = Document>,
        options: impl Into<Option<ChangeStreamOptions>>,
    ) -> Result<ChangeStream<ChangeStreamEvent<Document>>> {
        runtime::block_on(self.async_client.watch(pipeline, options)).map(ChangeStream::new)
    }

    /// Starts a new [`SessionChangeStream`] that receives events for all changes in the cluster
    /// using the provided [`ClientSession`].  See [`Client::watch`] for more information.
    pub fn watch_with_session(
        &self,
        pipeline: impl IntoIterator<Item = Document>,
        options: impl Into<Option<ChangeStreamOptions>>,
        session: &mut ClientSession,
    ) -> Result<SessionChangeStream<ChangeStreamEvent<Document>>> {
        runtime::block_on(self.async_client.watch_with_session(
            pipeline,
            options,
            &mut session.async_client_session,
        ))
        .map(SessionChangeStream::new)
    }
}
