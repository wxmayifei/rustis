#[cfg(feature = "redis-graph")]
use crate::commands::GraphCommands;
#[cfg(feature = "redis-json")]
use crate::commands::JsonCommands;
#[cfg(feature = "redis-search")]
use crate::commands::SearchCommands;
#[cfg(feature = "redis-time-series")]
use crate::commands::TimeSeriesCommands;
#[cfg(feature = "redis-bloom")]
use crate::commands::{
    BloomCommands, CountMinSketchCommands, CuckooCommands, TDigestCommands, TopKCommands,
};
use crate::{
    client::{
        Cache, ClientTrait, InnerClient, IntoConfig, Pipeline, PreparedCommand, PubSubStream,
        Transaction,
    },
    commands::{
        BitmapCommands, ClusterCommands, ConnectionCommands, GenericCommands, GeoCommands,
        HashCommands, HyperLogLogCommands, InternalPubSubCommands, ListCommands, PubSubCommands,
        ScriptingCommands, SentinelCommands, ServerCommands, SetCommands, SortedSetCommands,
        StreamCommands, StringCommands,
    },
    resp::{Command, FromValue, SingleArg, SingleArgOrCollection, Value},
    Future, Result,
};
use std::future::IntoFuture;

/// A multiplexed client that can be cloned, allowing requests
/// to be be sent concurrently on the same underlying connection.
///
/// Compared to a [single client](crate::client::Client), a multiplexed client cannot offers access
/// to all existing Redis commands.
///
/// Transactions and [blocking commands](crate::commands::BlockingCommands) are not compatible with a multiplexed client
/// because they monopolize the whole connection which cannot be shared anymore.
///
/// It means other consumers of the same
/// multiplexed client will be blocked each time a transaction or a blocking command is in progress, losing the advantage
/// of a shared connection.
///
/// ## See also
/// [Multiplexing Explained](https://redis.com/blog/multiplexing-explained/)
#[derive(Clone)]
pub struct MultiplexedClient {
    inner_client: InnerClient,
}

impl MultiplexedClient {
    /// Connects asynchronously to the Redis server.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the connection operation
    #[inline]
    pub async fn connect(config: impl IntoConfig) -> Result<Self> {
        let inner_client = InnerClient::connect(config).await?;
        Ok(Self { inner_client })
    }

    /// Send an arbitrary command to the Redis server.
    ///
    /// This is used primarily intended for implementing high level commands API
    /// but may also be used to provide access to new features that lack a direct API.
    ///
    /// # Arguments
    /// * `name` - Command name in uppercase.
    /// * `args` - Command arguments which can be provided as arrays (up to 4 elements) or vectors of [`CommandArg`](crate::resp::CommandArg).
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the send operation
    ///
    /// # Example
    /// ```
    /// use rustis::{client::MultiplexedClient, resp::cmd, Result};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let mut client = MultiplexedClient::connect("127.0.0.1:6379").await?;
    ///
    ///     let values: Vec<String> = client
    ///         .send(cmd("MGET").arg("key1").arg("key2").arg("key3").arg("key4"))
    ///         .await?
    ///         .into()?;
    ///     println!("{:?}", values);
    ///
    ///     Ok(())
    /// }
    /// ```
    #[inline]
    pub async fn send(&mut self, command: Command) -> Result<Value> {
        self.inner_client.send(command).await
    }

    /// Send command to the Redis server and forget its response.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the send operation

    #[inline]
    pub fn send_and_forget(&mut self, command: Command) -> Result<()> {
        self.inner_client.send_and_forget(command)
    }

    /// Send a command batch to the Redis server.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the send operation
    #[inline]
    pub async fn send_batch(&mut self, commands: Vec<Command>) -> Result<Value> {
        self.inner_client.send_batch(commands).await
    }

    /// Create a new pipeline
    #[inline]
    pub fn create_pipeline(&mut self) -> Pipeline {
        self.inner_client.create_pipeline()
    }

    /// Create a new transaction
    ///
    /// Because of the multiplexed nature of the client,
    /// [`watch`](crate::commands::TransactionCommands::watch) &
    /// [`unwatch`](crate::commands::TransactionCommands::unwatch)
    /// commands cannot be supported.
    /// To be able to use these commands with a transaction,
    /// [`Client`](crate::client::Client) or [`PooledClientManager`](crate::client::PooledClientManager)
    /// should be used instead
    #[inline]
    pub fn create_transaction(&mut self) -> Transaction {
        self.inner_client.create_transaction()
    }
}

impl ClientTrait for MultiplexedClient {
    #[inline]
    fn send(&mut self, command: Command) -> Future<Value> {
        Box::pin(async move { self.send(command).await })
    }

    #[inline]
    fn send_and_forget(&mut self, command: Command) -> Result<()> {
        self.send_and_forget(command)
    }

    #[inline]
    fn send_batch(&mut self, commands: Vec<Command>) -> Future<Value> {
        Box::pin(async move { self.send_batch(commands).await })
    }

    #[inline]
    fn create_pipeline(&mut self) -> Pipeline {
        self.create_pipeline()
    }

    #[inline]
    fn create_transaction(&mut self) -> Transaction {
        self.create_transaction()
    }

    #[inline]
    fn get_cache(&mut self) -> &mut Cache {
        self.inner_client.get_cache()
    }
}

/// Extension trait dedicated to [`PreparedCommand`](crate::client::PreparedCommand)
/// to add specific methods for the [`MultiplexedClient`](crate::client::MultiplexedClient) executor
pub trait MultiplexedPreparedCommand<'a, R>
where
    R: FromValue,
{
    /// Send command and forget its response
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occur during the send operation
    fn forget(self) -> Result<()>;
}

impl<'a, R> MultiplexedPreparedCommand<'a, R> for PreparedCommand<'a, MultiplexedClient, R>
where
    R: FromValue + Send + 'a,
{
    /// Send command and forget its response
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occur during the send operation
    #[inline]
    fn forget(self) -> Result<()> {
        self.executor.send_and_forget(self.command)
    }
}

impl<'a, R> IntoFuture for PreparedCommand<'a, MultiplexedClient, R>
where
    R: FromValue + Send + 'a,
{
    type Output = Result<R>;
    type IntoFuture = Future<'a, R>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if self.keep_command_for_result {
                let command_for_result = self.command.clone();
                self.executor
                    .send(self.command)
                    .await?
                    .into_with_command(&command_for_result)
            } else if let Some(post_process) = self.post_process {
                let command_for_result = self.command.clone();
                let result = self.executor.send(self.command).await?;
                post_process(result, command_for_result, self.executor).await
            } else {
                self.executor.send(self.command).await?.into()
            }
        })
    }
}

impl BitmapCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl BloomCommands for MultiplexedClient {}
impl ClusterCommands for MultiplexedClient {}
impl ConnectionCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl CountMinSketchCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl CuckooCommands for MultiplexedClient {}
impl GenericCommands for MultiplexedClient {}
impl GeoCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-graph")))]
#[cfg(feature = "redis-graph")]
impl GraphCommands for MultiplexedClient {}
impl HashCommands for MultiplexedClient {}
impl HyperLogLogCommands for MultiplexedClient {}
impl InternalPubSubCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-json")))]
#[cfg(feature = "redis-json")]
impl JsonCommands for MultiplexedClient {}
impl ListCommands for MultiplexedClient {}
impl ScriptingCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-search")))]
#[cfg(feature = "redis-search")]
impl SearchCommands for MultiplexedClient {}
impl SentinelCommands for MultiplexedClient {}
impl ServerCommands for MultiplexedClient {}
impl SetCommands for MultiplexedClient {}
impl SortedSetCommands for MultiplexedClient {}
impl StreamCommands for MultiplexedClient {}
impl StringCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl TDigestCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-time-series")))]
#[cfg(feature = "redis-time-series")]
impl TimeSeriesCommands for MultiplexedClient {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl TopKCommands for MultiplexedClient {}

impl PubSubCommands for MultiplexedClient {
    #[inline]
    fn subscribe<'a, C, CC>(&'a mut self, channels: CC) -> Future<'a, PubSubStream>
    where
        C: SingleArg + Send + 'a,
        CC: SingleArgOrCollection<C>,
    {
        self.inner_client.subscribe(channels)
    }

    #[inline]
    fn psubscribe<'a, P, PP>(&'a mut self, patterns: PP) -> Future<'a, PubSubStream>
    where
        P: SingleArg + Send + 'a,
        PP: SingleArgOrCollection<P>,
    {
        self.inner_client.psubscribe(patterns)
    }

    #[inline]
    fn ssubscribe<'a, C, CC>(&'a mut self, shardchannels: CC) -> Future<'a, PubSubStream>
    where
        C: SingleArg + Send + 'a,
        CC: SingleArgOrCollection<C>,
    {
        self.inner_client.ssubscribe(shardchannels)
    }
}
