/*!
Defines types related to the 3 clients structs and their dependencies:
[`Client`], [`MultiplexedClient`], and [`PooledClientManager`] and how to configure them

# Clients

The central object in **rustis** is the [`Client`](Client).

I will allow you to connect to the Redis server, to send command requests 
and to receive command response and push messages.

The [`Client`](Client) struct can be used in 3 different modes
* As a single client
* As a mutiplexer
* In a pool of clients

## The single client
The single [`Client`](crate::client::Client) maintains a unique connection to a Redis Server or cluster.

This use case of the client is not meant to be use directly in a Web application, where multiple HTTP connections access
The Redis server at the same time in a multi-thread architecture (like [Actix](https://actix.rs/) or [Rocket](https://rocket.rs/)).

It could be use in tools where the load is minimal.

```
use rustis::{
    client::Client,
    commands::{FlushingMode, ServerCommands, StringCommands},
    Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = Client::connect("127.0.0.1:6379").await?;
    client.flushdb(FlushingMode::Sync).await?;

    client.set("key", "value").await?;
    let value: String = client.get("key").await?;
    println!("value: {value:?}");

    Ok(())
}
```

## The multiplexer
A [`Client`](Client) instance can be cloned, allowing requests
to be be sent concurrently on the same underlying connection.

The multiplexer mode is great because it offers much performance in a multithread architecture, with only a single
underlying connections. It should be the prefered mode for Web applications.

### Limitations
Beware that using [`Client`](Client) in a multiplexer mode, by cloning an instance across multiple threads,
is not suitable for using [blocking commands](crate::commands::BlockingCommands) 
because they monopolize the whole connection which cannot be shared anymore.

Moreover using the [`watch`](crate::commands::TransactionCommands::watch) command is not compatible 
with the multiplexer mode is either. Indeed, it's the shared connection that will be watched, not only
the [`Client`](Client) instance through which the `watch`](crate::commands::TransactionCommands::watch) command is sent.

### Managing multiplexed subscriptions

Even if the [`subscribe`][crate::commands::PubSubCommands::subscribe] monopolize the whole connection, 
it is still possible to use it in a multiplexed [`Client`](Client). 

Indeed the subscribing mode of Redis still allows to share the connection between multiple clients,
at the only condition that this connection is dedicated to subscriptions.

In a Web application that requires subscriptions and regualar commands, the prefered solution
would be to connect two multiplexed clients to the Redis server:
* 1 for the subscriptions
* 1 for the regular commands

See also [Multiplexing Explained](https://redis.com/blog/multiplexing-explained/)

### Example
```
use rustis::{
    client::{Client, IntoConfig},
    commands::{FlushingMode, PubSubCommands, ServerCommands, StringCommands},
    Result
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = "127.0.0.1:6379".into_config()?;
    let mut regular_client1 = Client::connect(config.clone()).await?;
    let mut pub_sub_client = Client::connect(config).await?;

    regular_client1.flushdb(FlushingMode::Sync).await?;

    regular_client1.set("key", "value").await?;
    let value: String = regular_client1.get("key").await?;
    println!("value: {value:?}");

    // clone a second instance on the same underlying connection
    let mut regular_client2 = regular_client1.clone();
    let value: String = regular_client2.get("key").await?;
    println!("value: {value:?}");

    // use 2nd connection to manager subscriptions
    let mut pub_sub_stream = pub_sub_client.subscribe("my_channel").await?;
    pub_sub_stream.close().await?;

    Ok(())
}
```

## The pooled client manager
The pooled client manager holds a pool of [`Client`](Client)s, based on [bb8](https://docs.rs/bb8/latest/bb8/).

Each time a new command must be sent to the Redis Server, a client will be borrowed temporarily to the manager
and automatic given back to it at the end of the operation.

It is an alternative way to multiplexing, of managing **rustin** within a Web application.

The manager can be configured via [bb8](https://docs.rs/bb8/latest/bb8/) with a various of options like maximum size, maximum lifetime, etc.

For you convenience, [bb8](https://docs.rs/bb8/latest/bb8/) is reexported from the **rustis** crate.

```
use rustis::{
    client::PooledClientManager, commands::StringCommands, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    let manager = PooledClientManager::new("127.0.0.1:6379")?;
    let pool = rustis::bb8::Pool::builder()
        .max_size(10)
        .build(manager).await?;

    let mut client1 = pool.get().await.unwrap();
    client1.set("key1", "value1").await?;
    let value: String = client1.get("key1").await?;
    println!("value: {value:?}");

    let mut client2 = pool.get().await.unwrap();
    client2.set("key2", "value2").await?;
    let value: String = client2.get("key2").await?;
    println!("value: {value:?}");

    Ok(())
}
```

# Configuration

A [`Client`](Client) instance can be configured with the [`Config`](Config) struct:
* Authentication
* [`TlsConfig`](TlsConfig)
* [`ServerConfig`](ServerConfig) (Standalone, Sentinel or Cluster)

[`IntoConfig`] is a convenient trait to convert more known types to a [`Config`](Config) instance:
* &[`str`](https://doc.rust-lang.org/std/primitive.str.html)
* `(impl Into\<String\>, u16)`: a pair of host + port
* [`String`](https://doc.rust-lang.org/alloc/string/struct.String.html)
* [`Url`](https://docs.rs/url/latest/url/struct.Url.html)

## Url Syntax

The **rustis** [`Config`](Config) can also be built from an URL

### Standalone

```text
redis|rediss://[[<username>]:<password>@]<host>[:<port>][/<database>]
```

### Cluster

```text
redis|rediss[+cluster]://[[<username>]:<password>@]<host1>[:<port1>][,<host2>:[<port2>][,<hostN>:[<portN>]]]
```

### Sentinel

```text
redis|rediss[+sentinel]://[[<username>]:<password>@]<host>[:<port>]/<service>[/<database>]
                          [?wait_between_failures=<250>[&sentinel_username=<username>][&sentinel_password=<password>]]
```

`service` is the required name of the sentinel service

### Schemes
The URL scheme is used to detect the server type:
* `redis://`- Non secure TCP connection to a standalone Redis server
* `rediss://`- Secure (TSL) TCP connection to a standalone Redis server
* `redis+sentinel://`- Non secure TCP connection to a Redis sentinel network
* `rediss+sentinel://`- Secure (TSL) TCP connection to a Redis sentinel network
* `redis+cluster://`- Non secure TCP connection to a Redis cluster
* `rediss+cluster://`- Secure (TSL) TCP connection to a Redis cluster

### QueryParameters
Query parameters match perfectly optional configuration fields
of the struct [`Config`](Config) or its dependencies:
* `connect_timeout` - The time to attempt a connection before timing out (default 10,000ms).
* `wait_between_failures` - (Sentinel only) Waiting time after failing before connecting to the next Sentinel instance (default 250ms).
* `sentinel_username` - (Sentinel only) Sentinel username
* `sentinel_password` - (Sentinel only) Sentinel password

### Example

```
use rustis::{client::Client, resp::cmd, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // standalone, host=localhost, port=6379 (default), database=1
    let mut client = Client::connect("redis://localhost/1").await?;

    Ok(())
}
```

# Pipelining

One of the most performant Redis server is [pipelining](https://redis.io/docs/manual/pipelining/).
This allow to optimize round-trip times by batching Redis commands.

### API description

You can create a pipeline on a [`Client`](Client) instance by calling the associated fonction [`create_pipeline`](Client::create_pipeline).
Be sure to store the pipeline instance in a mutable variable because a pipeline requires an exclusive access.

Once the pipeline is created, you can use exactly the same commands that you would directly use on a client instance.
This is possible because the [`Pipeline`](Pipeline) implements all the built-in [command traits](crate::commands).

The main difference, is that you have to choose for each command:
* to [`queue`](BatchPreparedCommand::queue) it, meaning that the [`Pipeline`](Pipeline) instance will queue the command in an internal
  queue to be able to send later the batch of commands to the Redis server.
* to [`forget`](BatchPreparedCommand::forget) it, meaning that the command will be queued as well BUT its response won't be awaited
  by the [`Pipeline`](Pipeline) instance

Finally, call the [`execute`](Pipeline::execute) associated function.

It is the caller responsability to use the right type to cast the server response
to the right tuple or collection depending on which command has been
[queued](BatchPreparedCommand::queue) or [forgotten](BatchPreparedCommand::forget).

The most generic type that can be requested as a result is `Vec<resp::Value>`

### Example
```
use rustis::{
    client::{Client, Pipeline, BatchPreparedCommand},
    commands::StringCommands,
    resp::{cmd, Value}, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = Client::connect("127.0.0.1:6379").await?;

    let mut pipeline = client.create_pipeline();
    pipeline.set("key1", "value1").forget();
    pipeline.set("key2", "value2").forget();
    pipeline.queue(cmd("UNKNOWN"));
    pipeline.get::<_, String>("key1").queue();
    pipeline.get::<_, String>("key2").queue();

    let (result, value1, value2): (Value, String, String) = pipeline.execute().await?;
    assert!(matches!(result, Value::Error(_)));
    assert_eq!("value1", value1);
    assert_eq!("value2", value2);

    Ok(())
}
```

# Transactions
[Redis Transactions](https://redis.io/docs/manual/transactions/) allow the execution of a group of commands in a single step.

All the commands in a transaction are serialized and executed sequentially.
A request sent by another client will never be served in the middle of the execution of a Redis Transaction.
This guarantees that the commands are executed as a single isolated operation.

### API description

You can create a transaction on a client instance by calling the associated fonction [`create_transaction`](Client::create_transaction).
Be sure to store the transaction instance in a mutable variable because a transaction requires an exclusive access.

Once the transaction is created, you can use exactly the same commands that you would directly use on a client instance.
This is possible because the [`Transaction`](Transaction) implements all the built-in [command traits](crate::commands).

The main difference, is that you have to choose for each command:
* to [`queue`](BatchPreparedCommand::queue) it, meaning that the [`Transaction`](Transaction) instance will queue the command in an internal
  queue to be able to send later the batch of commands to the Redis server.
* to [`forget`](BatchPreparedCommand::forget) it, meaning that the command will be queued as well BUT its response won't be awaited
  by the [`Transaction`](Transaction) instance.

Finally, call the [`execute`](Transaction::execute) associated function.

It is the caller responsability to use the right type to cast the server response
to the right tuple or collection depending on which command has been
[queued](BatchPreparedCommand::queue) or [forgotten](BatchPreparedCommand::forget).

The most generic type that can be requested as a result is `Vec<(resp::Value)>`

### Example
```
use rustis::{
    client::{Client, Transaction, BatchPreparedCommand},
    commands::StringCommands,
    resp::{cmd, Value}, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = Client::connect("127.0.0.1:6379").await?;

    let mut transaction = client.create_transaction();

    transaction.set("key1", "value1").forget();
    transaction.set("key2", "value2").forget();
    transaction.get::<_, String>("key1").queue();
    let value: String = transaction.execute().await?;

    assert_eq!("value1", value);

    Ok(())
}
```

# Pub/Sub
[`Pub/Sub`](https://redis.io/docs/manual/pubsub/) is a Redis architecture were senders can publish messages into channels
and subscribers can subscribe by channel names or patterns to receive messages

### Publishing

To publish a message, you can call the [`publish`](crate::commands::PubSubCommands::publish)
associated function on its dedicated trait.

It also possible to use the sharded flavor of the publish function: [`spublish`](crate::commands::PubSubCommands::spublish)

### Subscribing

Subscribing is blocking the current client connection, in order to let the client wait for incoming messages.
Consequently, **rustis** implements subsribing through an async [`Stream`](https://docs.rs/futures/latest/futures/stream/trait.Stream.html).

You can create a [`PubSubStream`](PubSubStream) by calling [`subscribe`](crate::commands::PubSubCommands::subscribe),
[`psubscribe`](crate::commands::PubSubCommands::psubscribe), or [`ssubscribe`](crate::commands::PubSubCommands::ssubscribe)
on their dedicated crate.

Then by calling [`next`](https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html#method.next) on the pub/sub stream, you can
wait for incoming message in the form of the struct [`PubSubMessage`](crate::client::PubSubMessage).

### Warning!

[`MultiplexedClient`](MultiplexedClient) instances must be decidated to Pub/Sub once a subscribing function has been called.
Indeed, because subscription blocks the multiplexed client shared connection,
other callers would be blocked when sending regular commands.

### Example

```
use rustis::{
    client::{Client, ClientPreparedCommand},
    commands::{FlushingMode, PubSubCommands, ServerCommands},
    resp::{cmd, Value}, Result,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let mut subscribing_client = Client::connect("127.0.0.1:6379").await?;
    let mut regular_client = Client::connect("127.0.0.1:6379").await?;

    // cleanup
    regular_client.flushdb(FlushingMode::Sync).await?;

    // subscribing_client subscribes
    let mut pub_sub_stream = subscribing_client.subscribe("mychannel").await?;

    // regular_client publishes
    regular_client.publish("mychannel", "mymessage").await?;

    // subscribing_client wait for the next message
    if let Some(message) = pub_sub_stream.next().await {
        let mut message = message?;
        let channel: String = message.get_channel()?;
        let payload: String = message.get_payload()?;

        assert_eq!("mychannel", channel);
        assert_eq!("mymessage", payload);
    }

    pub_sub_stream.close().await?;

    Ok(())
}
```

### Additional Subscriptions

Once the stream has been created, it is still possible to add addtional subscriptions
by calling [`subscribe`](PubSubStream::subscribe), [`psubscribe`](PubSubStream::psubscribe)
or [`ssubscribe`](PubSubStream::ssubscribe) on the [`PubSubStream`](PubSubStream) instance

#### Example

```
use rustis::{
    client::{Client, ClientPreparedCommand},
    commands::{FlushingMode, PubSubCommands, ServerCommands},
    resp::{cmd, Value}, Result,
};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    let mut subscribing_client = Client::connect("127.0.0.1:6379").await?;

    // 1st subscription
    let mut pub_sub_stream = subscribing_client.subscribe("mychannel1").await?;

    // 2nd subscription
    pub_sub_stream.subscribe("mychannel2").await?;

    // 3nd subscription (possibility to mix all the kinds of subscription)
    pub_sub_stream.psubscribe("o*").await?;

    pub_sub_stream.close().await?;

    Ok(())
}
```
*/

mod client_state;
#[allow(clippy::module_inception)]
mod client;
mod config;
mod message;
mod monitor_stream;
mod pipeline;
#[cfg_attr(docsrs, doc(cfg(feature = "pool")))]
#[cfg(feature = "pool")]
mod pooled_client_manager;
mod prepared_command;
mod pub_sub_stream;
mod transaction;

pub use client_state::*;
pub use client::*;
pub use config::*;
pub(crate) use message::*;
pub use monitor_stream::*;
pub use pipeline::*;
#[cfg_attr(docsrs, doc(cfg(feature = "pool")))]
#[cfg(feature = "pool")]
pub use pooled_client_manager::*;
pub use prepared_command::*;
pub use pub_sub_stream::*;
pub use transaction::*;