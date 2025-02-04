use dashmap::DashMap;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};

/// A message that can be published through the message router.
///
/// Contains a topic string that determines which subscribers receive the message,
/// and a generic payload of type `T` that must implement `Clone`.
#[derive(Debug, Clone)]
pub struct Message<T: Clone> {
    topic: String,
    payload: T,
}

impl<T: Clone> Message<T> {
    /// Creates a new message with the specified topic and payload.
    ///
    /// # Parameters
    /// * `topic` - The topic identifier for this message. Will be converted to a String.
    /// * `payload` - The message payload of type `T`
    ///
    /// # Returns
    /// A new `Message<T>` instance
    pub fn new(topic: impl Into<String>, payload: T) -> Self {
        Self {
            topic: topic.into(),
            payload,
        }
    }

    /// Returns a reference to the message's topic string.
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Returns a reference to the message's payload.
    pub fn payload(&self) -> &T {
        &self.payload
    }
}

/// A pub/sub message router that allows publishing messages to topics and subscribing to receive them.
///
/// The router maintains a list of subscribers for each topic and delivers messages to all subscribers
/// of the topic they were published to. Messages contain a generic payload type `T` that must implement `Clone`.
///
/// Messages are delivered asynchronously through tokio channels with a buffer size of 32 messages.
#[derive(Default)]
pub struct MessageRouter<T: Clone> {
    subscribers: Arc<DashMap<String, Vec<Sender<Message<T>>>>>,
}

impl<T: Clone> MessageRouter<T> {
    /// Creates a new empty message router.
    ///
    /// # Returns
    /// A new `MessageRouter<T>` instance with no subscribers.
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(DashMap::new()),
        }
    }

    /// Subscribes to messages published to the specified topic.
    ///
    /// Creates a new channel with buffer size 32 and registers the sender in the router.
    /// Returns the receiver end of the channel which will receive all messages published
    /// to the specified topic.
    ///
    /// # Parameters
    /// * `topic` - The topic to subscribe to. Will be converted to a String.
    ///
    /// # Returns
    /// A `Receiver<Message<T>>` that will receive messages published to the topic
    pub async fn subscribe(&self, topic: impl Into<String>) -> Receiver<Message<T>> {
        let topic = topic.into();
        let (tx, rx) = mpsc::channel(32);

        self.subscribers
            .entry(topic)
            .or_insert_with(Vec::new)
            .push(tx);

        rx
    }

    /// Publishes a message to all subscribers of its topic.
    ///
    /// The message will be cloned and sent to each subscriber of the topic. If a send
    /// fails (e.g., because the receiver was dropped), the error is ignored and delivery
    /// continues to remaining subscribers.
    ///
    /// If there are no subscribers for the topic, the message is silently dropped.
    ///
    /// # Parameters
    /// * `message` - The message to publish
    pub async fn publish(&self, message: Message<T>) {
        if let Some(topic_subscribers) = self.subscribers.get(&message.topic) {
            for subscriber in topic_subscribers.deref() {
                let _ = subscriber.send(message.clone()).await;
            }
        }
    }

    /// Removes closed subscriber channels and empty topics.
    ///
    /// This method should be called periodically to clean up resources associated with
    /// dropped subscribers. It:
    /// - Removes all closed sender channels from topic subscriber lists
    /// - Removes any topics that no longer have any subscribers
    pub async fn cleanup(&self) {
        self.subscribers.retain(|_, senders| {
            senders.retain(|sender| !sender.is_closed());
            !senders.is_empty()
        });
    }

    /// Returns the number of subscribers for a given topic.
    ///
    /// This method is only available in test builds and is used for testing the router's behavior.
    ///
    /// # Parameters
    /// * `topic` - The topic to count subscribers for
    ///
    /// # Returns
    /// The number of subscribers currently registered for the topic
    #[cfg(test)]
    async fn subscriber_count(&self, topic: &str) -> usize {
        self.subscribers.get(topic).map_or(0, |v| v.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_subscribe_single_topic() {
        let router = MessageRouter::<String>::new();
        let _subscriber = router.subscribe("test").await;
        assert_eq!(router.subscriber_count("test").await, 1);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let router = MessageRouter::<String>::new();
        let _sub1 = router.subscribe("test").await;
        let _sub2 = router.subscribe("test").await;
        let _sub3 = router.subscribe("other").await;

        assert_eq!(router.subscriber_count("test").await, 2);
        assert_eq!(router.subscriber_count("other").await, 1);
    }

    #[tokio::test]
    async fn test_message_delivery() {
        let router = MessageRouter::<String>::new();
        let mut subscriber = router.subscribe("test").await;

        // Spawn a task to receive the message
        let receive_task = tokio::spawn(async move {
            if let Ok(Some(msg)) = timeout(Duration::from_secs(1), subscriber.recv()).await {
                return msg.payload().to_string();
            }
            String::from("timeout")
        });

        // Send a message
        router
            .publish(Message::new("test", "hello".to_string()))
            .await;

        // Wait for the result
        let result = receive_task.await.unwrap();
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn test_multi_topic_delivery() {
        let router = MessageRouter::<i32>::new();
        let mut sub1 = router.subscribe("numbers").await;
        let mut sub2 = router.subscribe("numbers").await;
        let mut sub3 = router.subscribe("other").await;

        router.publish(Message::new("numbers", 42)).await;
        router.publish(Message::new("other", 100)).await;

        assert_eq!(sub1.recv().await.unwrap().payload(), &42);
        assert_eq!(sub2.recv().await.unwrap().payload(), &42);
        assert_eq!(sub3.recv().await.unwrap().payload(), &100);
    }

    #[tokio::test]
    async fn test_cleanup_removes_closed_subscribers() {
        let router = MessageRouter::<String>::new();
        let sub1 = router.subscribe("test").await;
        let _sub2 = router.subscribe("test").await;

        // Drop one subscriber
        drop(sub1);

        // Cleanup should remove the closed subscriber
        router.cleanup().await;
        assert_eq!(router.subscriber_count("test").await, 1);
    }

    #[tokio::test]
    async fn test_cleanup_removes_empty_topics() {
        let router = MessageRouter::<String>::new();
        let sub = router.subscribe("test").await;

        // Initially has one subscriber
        assert_eq!(router.subscriber_count("test").await, 1);

        // Drop the only subscriber
        drop(sub);

        // Cleanup should remove the entire topic
        router.cleanup().await;
        assert_eq!(router.subscriber_count("test").await, 0);
    }

    #[tokio::test]
    async fn test_publish_to_nonexistent_topic() {
        let router = MessageRouter::<String>::new();

        // This should not panic
        router
            .publish(Message::new("nonexistent", "test".to_string()))
            .await;
    }

    #[tokio::test]
    async fn test_custom_type() {
        #[derive(Debug, Clone, PartialEq)]
        struct TestData {
            value: i32,
            name: String,
        }

        let router = MessageRouter::<TestData>::new();
        let mut subscriber = router.subscribe("test").await;

        let test_data = TestData {
            value: 42,
            name: "test".to_string(),
        };

        router
            .publish(Message::new("test", test_data.clone()))
            .await;

        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.payload(), &test_data);
    }
}
