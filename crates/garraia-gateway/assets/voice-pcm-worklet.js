// The microphone never leaves this processor until the UI has an active call.
// A fractional box resampler keeps 44.1/48 kHz devices on the same 16 kHz wire
// format without requiring a particular AudioContext sample rate.
class GarraVoicePcm extends AudioWorkletProcessor {
  constructor() {
    super();
    this.ratio = sampleRate / 16000;
    this.sum = 0;
    this.weight = 0;
    this.frame = new Float32Array(320);
    this.offset = 0;
    this.live = true;
    this.port.onmessage = ({ data }) => { if (data === 'stop') this.live = false; };
  }

  process(inputs) {
    if (!this.live) return false;
    const channels = inputs[0];
    if (!channels || !channels[0]) return true;
    for (let i = 0; i < channels[0].length; i++) {
      let mono = 0;
      for (const channel of channels) mono += channel[i] || 0;
      mono /= channels.length;
      let remaining = 1;
      while (remaining > 1e-8) {
        const take = Math.min(remaining, this.ratio - this.weight);
        this.sum += mono * take;
        this.weight += take;
        remaining -= take;
        if (this.weight + 1e-8 >= this.ratio) {
          this.frame[this.offset++] = this.sum / this.ratio;
          this.sum = this.weight = 0;
          if (this.offset === this.frame.length) {
            let energy = 0;
            for (const value of this.frame) energy += value * value;
            this.port.postMessage({ samples: this.frame, rms: Math.sqrt(energy / 320) }, [this.frame.buffer]);
            this.frame = new Float32Array(320);
            this.offset = 0;
          }
        }
      }
    }
    // Output is intentionally silent. Connecting it to destination makes
    // processing reliable across browsers without monitoring the microphone.
    return true;
  }
}

registerProcessor('garra-voice-pcm', GarraVoicePcm);
