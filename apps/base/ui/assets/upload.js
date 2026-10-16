// Browser half of the live file input (templates/forms.html `file_input`,
// src/upload.rs): Bilimbi's Phoenix.LiveFileUpload hook and LiveUploader,
// ported to the Bilibi transport. The server owns the entry list, the rules
// (accept, max entries, max file size) and every byte received; this hook owns
// only the File objects the person chose and the stream that carries them.
//
//   * Choosing files announces them through the transport as an `upload`
//     intent (BilibiLive.send). The reply names, per file, the id to stream it
//     under or the error it was refused with; the server then re-renders the
//     owner, so the rows appear from its state.
//   * Accepted files are streamed as binary frames of `u32 id, u32 offset,
//     bytes`, one file at a time, one chunk in flight: the next chunk goes
//     when the server acknowledges the last (a `chunk` message, relayed here
//     as the window event `bilibi:upload`). The hook writes the row's
//     percentage between the server's own renders; a patch rewrites it from
//     the server's count, which is the truth.
//   * Remove sends `upload_cancel`; the server drops the entry and re-renders.
//   * A submit while a file is still arriving is held and re-raised once
//     every file has arrived, as LiveView waits for its uploads before the
//     submit event. The owner consumes only completed files, so nothing
//     half-received can reach it either way.
//   * A dropped socket ends every stream: the remounted state holds no bytes,
//     and the rows it renders say so.
(() => {
  'use strict';
  let refs = 0;
  const HEADER = 8;

  function ownerTarget(el) {
    const owner = el.closest('[data-target]');
    return owner ? owner.dataset.target : 'page';
  }

  const FileUpload = {
    mounted() {
      this.pending = new Map(); // id -> {file, offset, size, chunk, inflight}
      this.announced = 0; // choices the server has not yet answered
      this.generation = 0;
      this.resume = null;
      this.wrapper = this.el.closest('[data-file-input]') || this.el.parentElement;
      this.onChange = () => this.select([...this.el.files]);
      this.onAck = (event) => this.acked(event.detail || {});
      this.onDrop = () => this.reset();
      this.onClick = (event) => {
        const button = event.target.closest('button[name="upload-cancel"]');
        if (!button || !this.wrapper.contains(button) || button.disabled) return;
        this.cancel(Number(button.value), button);
      };
      this.onSubmit = (event) => {
        if (!this.busy()) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        this.resume = {submitter: event.submitter || null};
        this.el.form.setAttribute('aria-busy', 'true');
      };
      this.el.addEventListener('change', this.onChange);
      this.wrapper.addEventListener('click', this.onClick);
      window.addEventListener('bilibi:upload', this.onAck);
      window.addEventListener('bilibi:disconnected', this.onDrop);
      this.form = this.el.form;
      if (this.form) this.form.addEventListener('submit', this.onSubmit, true);
    },

    destroyed() {
      this.el.removeEventListener('change', this.onChange);
      this.wrapper.removeEventListener('click', this.onClick);
      window.removeEventListener('bilibi:upload', this.onAck);
      window.removeEventListener('bilibi:disconnected', this.onDrop);
      if (this.form) this.form.removeEventListener('submit', this.onSubmit, true);
      this.pending.clear();
    },

    select(files) {
      if (!files.length || !window.BilibiLive) return;
      const held = new Map();
      const entries = files.map((file) => {
        const ref = String(++refs);
        held.set(ref, file);
        return {ref, name: file.name || ref, size: file.size, content_type: file.type || ''};
      });
      // The input forgets its selection so the same file can be chosen again
      // after a Remove; the rows the server renders are the record of it.
      this.el.value = '';
      const generation = this.generation;
      const target = ownerTarget(this.el);
      this.announced += 1;
      window.BilibiLive.send({type: 'upload', target, upload: this.el.dataset.upload, entries}, {
        source: this.el,
        onReply: (reply) => {
          if (generation === this.generation) this.announced = Math.max(0, this.announced - 1);
          if (reply.type !== 'upload') { this.settle(); return; }
          for (const registration of reply.entries || []) {
            if (registration.id == null) continue;
            // Files announced before a drop are gone with the old state.
            if (generation !== this.generation) {
              window.BilibiLive.send({type: 'upload_cancel', target, id: registration.id});
              continue;
            }
            const file = held.get(registration.ref);
            if (!file || registration.done) continue;
            this.pending.set(registration.id, {file, offset: 0, size: file.size, chunk: registration.chunk || 65536, inflight: false});
          }
          this.pump();
          this.settle();
        },
      });
    },

    // One file at a time, one chunk in flight, in the order the files were
    // chosen; the server processes a socket sequentially either way.
    pump() {
      if ([...this.pending.values()].some((entry) => entry.inflight)) return;
      const next = this.pending.entries().next();
      if (next.done) return;
      const [id, entry] = next.value;
      entry.inflight = true;
      const end = Math.min(entry.offset + entry.chunk, entry.size);
      entry.file.slice(entry.offset, end).arrayBuffer().then((buffer) => {
        if (!this.pending.has(id) || this.pending.get(id) !== entry) return;
        const frame = new Uint8Array(HEADER + buffer.byteLength);
        const view = new DataView(frame.buffer);
        view.setUint32(0, id);
        view.setUint32(4, entry.offset);
        frame.set(new Uint8Array(buffer), HEADER);
        if (!window.BilibiLive.sendBytes(frame)) this.reset();
      }, () => {
        // The file changed or vanished under the person; the server marks the
        // entry interrupted when the stream stops, and the row says so.
        this.pending.delete(id);
        this.pump();
        this.settle();
      });
    },

    acked(detail) {
      const entry = this.pending.get(detail.id);
      if (!entry) return;
      if (detail.error) {
        this.pending.delete(detail.id);
      } else {
        entry.offset = detail.received;
        entry.inflight = false;
        this.paint(detail.id, detail.received, entry.size);
        if (detail.done) this.pending.delete(detail.id);
      }
      this.pump();
      this.settle();
    },

    paint(id, received, size) {
      const row = this.wrapper.querySelector(`[data-upload-entry="${id}"] [data-upload-progress]`);
      if (!row) return;
      const text = `${size ? Math.min(100, Math.floor(received * 100 / size)) : 100}%`;
      if (row.textContent !== text) row.textContent = text;
    },

    cancel(id, button) {
      this.pending.delete(id);
      window.BilibiLive.send({type: 'upload_cancel', target: ownerTarget(this.el), id}, {source: button});
      this.pump();
      this.settle();
    },

    // Files are busy from the moment they are chosen: a submit raised before
    // the server has even answered the choice waits like one raised mid-stream.
    busy() {
      return this.pending.size > 0 || this.announced > 0;
    },

    // The held submit goes once every stream has ended, whatever ended it.
    settle() {
      if (!this.resume || this.busy()) return;
      const {submitter} = this.resume;
      this.resume = null;
      if (!this.form) return;
      this.form.removeAttribute('aria-busy');
      if (submitter && submitter.isConnected && submitter.form === this.form) this.form.requestSubmit(submitter);
      else this.form.requestSubmit();
    },

    reset() {
      this.generation += 1;
      this.pending.clear();
      this.announced = 0;
      this.resume = null;
      if (this.form) this.form.removeAttribute('aria-busy');
    },
  };

  window.BilibiUI.registerHook('FileUpload', FileUpload);
})();
