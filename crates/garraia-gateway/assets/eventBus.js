export const EventBus = {
  events: {},
  on(event, listener) {
    if (!this.events[event]) this.events[event] = [];
    this.events[event].push(listener);
  },
  emit(event, data) {
    if (this.events[event]) {
      this.events[event].forEach((listener) => {
        try {
          listener(data);
        } catch (e) {
          console.error(`Error in EventBus listener for ${event}:`, e);
        }
      });
    }
  },
};
