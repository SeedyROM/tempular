use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Message<T: Clone> {
    topic: String,
    payload: T,
}

impl<T: Clone> Message<T> {
    pub fn new(topic: impl Into<String>, payload: T) -> Self {
        Self {
            topic: topic.into(),
            payload,
        }
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub fn payload(&self) -> &T {
        &self.payload
    }
}

pub struct MessageRouter<T: Clone> {
    subscribers: Arc<Mutex<HashMap<String, Vec<Sender<Message<T>>>>>>,
}

impl<T: Clone> MessageRouter<T> {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn subscribe(&self, topic: impl Into<String>) -> Receiver<Message<T>> {
        let topic = topic.into();
        let (tx, rx) = mpsc::channel(32);

        let mut subscribers = self.subscribers.lock().await;
        subscribers.entry(topic).or_insert_with(Vec::new).push(tx);

        rx
    }

    pub async fn publish(&self, message: Message<T>) {
        let subscribers = self.subscribers.lock().await;

        if let Some(topic_subscribers) = subscribers.get(&message.topic) {
            for subscriber in topic_subscribers {
                let _ = subscriber.send(message.clone()).await;
            }
        }
    }

    pub async fn cleanup(&self) {
        let mut subscribers = self.subscribers.lock().await;

        subscribers.retain(|_, senders| {
            senders.retain(|sender| !sender.is_closed());
            !senders.is_empty()
        });
    }

    // Helper method for testing - returns number of subscribers for a topic
    #[cfg(test)]
    async fn subscriber_count(&self, topic: &str) -> usize {
        let subscribers = self.subscribers.lock().await;
        subscribers.get(topic).map_or(0, |v| v.len())
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
