#[cfg(feature = "redis-graph")]
use crate::commands::GraphCommands;
#[cfg(feature = "redis-json")]
use crate::commands::JsonCommands;
#[cfg(feature = "redis-search")]
use crate::commands::SearchCommands;
#[cfg(feature = "redis-time-series")]
use crate::commands::TimeSeriesCommands;
use crate::{
    client::{
        Cache, ClientTrait, InnerClient, IntoConfig, Message, MonitorStream,
        Pipeline, PreparedCommand, PubSubStream, Transaction,
    },
    commands::{
        BitmapCommands, BlockingCommands, ClusterCommands, ConnectionCommands, GenericCommands,
        GeoCommands, HashCommands, HyperLogLogCommands, InternalPubSubCommands, ListCommands,
        PubSubCommands, ScriptingCommands, SentinelCommands, ServerCommands, SetCommands,
        SortedSetCommands, StreamCommands, StringCommands, TransactionCommands,
    },
    network::{MonitorReceiver, MonitorSender},
    resp::{cmd, Command, CommandArg, FromValue, ResultValueExt, SingleArgOrCollection, Value},
    Future, Result, ValueReceiver, ValueSender,
};
#[cfg(feature = "redis-bloom")]
use crate::commands::{BloomCommands, CountMinSketchCommands, CuckooCommands, TDigestCommands, TopKCommands};
use futures::channel::{mpsc, oneshot};
use std::future::IntoFuture;

/// Client with a unique connection to a Redis server.
pub struct Client {
    inner_client: InnerClient,
}

impl Client {
    /// Connects asynchronously to the Redis server.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the connection operation
    #[inline]
    pub async fn connect(config: impl IntoConfig) -> Result<Self> {
        let inner_client = InnerClient::connect(config).await?;
        Ok(Self { inner_client })
    }

    /// We don't want the Client struct to be publicly cloneable
    /// If one wants to consume a multiplexed client,
    /// the [MultiplexedClient](crate::client::MultiplexedClient) must be used instead
    #[inline]
    pub(crate) fn clone(&self) -> Client {
        Client {
            inner_client: self.inner_client.clone(),
        }
    }

    /// Send an arbitrary command to the server.
    ///
    /// This is used primarily intended for implementing high level commands API
    /// but may also be used to provide access to new features that lack a direct API.
    ///
    /// # Arguments
    /// * `command` - generic [`Command`](crate::resp::Command) meant to be sent to the Redis server.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the send operation
    ///
    /// # Example
    /// ```
    /// use rustis::{client::Client, resp::cmd, Result};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let mut client = Client::connect("127.0.0.1:6379").await?;
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

    /// Send a batch of commands to the Redis server.
    ///
    /// # Arguments
    /// * `commands` - batch of generic [`Command`](crate::resp::Command)s meant to be sent to the Redis server.
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occurs during the send operation
    #[inline]
    pub async fn send_batch(&mut self, commands: Vec<Command>) -> Result<Value> {
        self.inner_client.send_batch(commands).await
    }

    /// Create a new transaction
    #[inline]
    pub fn create_transaction(&mut self) -> Transaction {
        self.inner_client.create_transaction()
    }

    /// Create a new pipeline
    #[inline]
    pub fn create_pipeline(&mut self) -> Pipeline {
        self.inner_client.create_pipeline()
    }
}

impl ClientTrait for Client {
    
    #[inline]fn send(&mut self, command: Command) -> Future<Value> {
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
/// to add specific methods for the [`Client`](crate::client::Client) executor
pub trait ClientPreparedCommand<'a, R>
where
    R: FromValue,
{
    /// Send command and forget its response
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occur during the send operation
    fn forget(self) -> Result<()>;
}

impl<'a, R> ClientPreparedCommand<'a, R> for PreparedCommand<'a, Client, R>
where
    R: FromValue + Send + 'a,
{
    /// Send command and forget its response
    ///
    /// # Errors
    /// Any Redis driver [`Error`](crate::Error) that occur during the send operation
    fn forget(self) -> Result<()> {
        self.executor.send_and_forget(self.command)
    }
}

impl<'a, R> IntoFuture for PreparedCommand<'a, Client, R>
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

impl BitmapCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl BloomCommands for Client {}
impl ClusterCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl CountMinSketchCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl CuckooCommands for Client {}
impl ConnectionCommands for Client {}
impl GenericCommands for Client {}
impl GeoCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-graph")))]
#[cfg(feature = "redis-graph")]
impl GraphCommands for Client {}
impl HashCommands for Client {}
impl HyperLogLogCommands for Client {}
impl InternalPubSubCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-json")))]
#[cfg(feature = "redis-json")]
impl JsonCommands for Client {}
impl ListCommands for Client {}
impl ScriptingCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-search")))]
#[cfg(feature = "redis-search")]
impl SearchCommands for Client {}
impl SentinelCommands for Client {}
impl ServerCommands for Client {}
impl SetCommands for Client {}
impl SortedSetCommands for Client {}
impl StreamCommands for Client {}
impl StringCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl TDigestCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-time-series")))]
#[cfg(feature = "redis-time-series")]
impl TimeSeriesCommands for Client {}
impl TransactionCommands for Client {}
#[cfg_attr(docsrs, doc(cfg(feature = "redis-bloom")))]
#[cfg(feature = "redis-bloom")]
impl TopKCommands for Client {}

impl PubSubCommands for Client {
    #[inline]
    fn subscribe<'a, C, CC>(&'a mut self, channels: CC) -> Future<'a, PubSubStream>
    where
        C: Into<CommandArg> + Send + 'a,
        CC: SingleArgOrCollection<C>,
    {
        self.inner_client.subscribe(channels)
    }

    #[inline]
    fn psubscribe<'a, P, PP>(&'a mut self, patterns: PP) -> Future<'a, PubSubStream>
    where
        P: Into<CommandArg> + Send + 'a,
        PP: SingleArgOrCollection<P>,
    {
        self.inner_client.psubscribe(patterns)
    }

    #[inline]
    fn ssubscribe<'a, C, CC>(&'a mut self, shardchannels: CC) -> Future<'a, PubSubStream>
    where
        C: Into<CommandArg> + Send + 'a,
        CC: SingleArgOrCollection<C>,
    {
        self.inner_client.ssubscribe(shardchannels)
    }
}

impl BlockingCommands for Client {
    fn monitor(&mut self) -> Future<MonitorStream> {
        Box::pin(async move {
            let (value_sender, value_receiver): (ValueSender, ValueReceiver) = oneshot::channel();
            let (monitor_sender, monitor_receiver): (MonitorSender, MonitorReceiver) =
                mpsc::unbounded();

            let message = Message::monitor(cmd("MONITOR"), value_sender, monitor_sender);

            self.inner_client.send_message(message)?;

            let value = value_receiver.await?;
            value.map_into_result(|_| MonitorStream::new(monitor_receiver, self.clone()))
        })
    }
}
