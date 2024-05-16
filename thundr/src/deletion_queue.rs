/// Timeline based deltion queue
///
/// Austin Shafer - 2024
use crate::Droppable;

/// A queue of items to be dropped for a particular timeline point.
struct DQTimelinePointQueue {
    pq_sync_point: u64,
    pq_items: Vec<Box<dyn Droppable + Send + Sync>>,
}

/// A timeline point based deletion queue for Device
///
/// This schedules items to be dropped when a certain timeline
/// point is hit. This timeline point will refer to the Device's
/// graphics timeline.
pub struct DeletionQueue {
    /// The last timeline point that we have already seen
    /// If new items are not newer than this point then they are dropped
    /// immediately instead of being added to any queue.
    dq_last_signaled: u64,
    /// A queue per timeline point
    dq_point_queues: Vec<DQTimelinePointQueue>,
}

impl DeletionQueue {
    /// Creates an empty deletion queue
    pub fn new() -> Self {
        Self {
            dq_last_signaled: 0,
            dq_point_queues: Vec::new(),
        }
    }

    /// Schedule the item to be dropped once the specified timeline
    /// point has passed.
    ///
    /// This does not drop the item immediately, unless the timeline point
    /// is already known to be signaled.
    pub fn schedule_drop_at_point(
        &mut self,
        item: Box<dyn Droppable + Send + Sync>,
        sync_point: u64,
    ) {
        if sync_point <= self.dq_last_signaled {
            return;
        }

        // Add this item to an existing queue, if there is one for this sync point
        if let Some(point_queue) = self
            .dq_point_queues
            .iter_mut()
            .find(|pq| pq.pq_sync_point == sync_point)
        {
            point_queue.pq_items.push(item);
            return;
        }

        // Otherwise create a new queue that contains this item
        self.dq_point_queues.push(DQTimelinePointQueue {
            pq_sync_point: sync_point,
            pq_items: vec![item],
        });
    }

    /// Release all pending items for a timeline point
    ///
    /// This clears all deletion queues for this sync point, including
    /// sync points preceeding this one.
    pub fn drop_all_at_point(&mut self, sync_point: u64) {
        self.dq_last_signaled = sync_point;

        self.dq_point_queues
            .retain(|pq| pq.pq_sync_point <= sync_point);
    }
}
