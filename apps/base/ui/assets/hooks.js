// Browser behaviour for the shared form controls in templates/forms.html:
// CodeInput, Combobox, SecretReveal, SecretClear, SecretStored and
// DisclosureDismiss, adapted from Bilimbi's Phoenix hooks (apps/web/assets/js
// at the pinned source) with the LiveView JS commands folded into the hooks.
// Each registers with the shared `data-hook` runtime in ui.js
// (BilibiUI.registerHook), which mounts a control when it enters the
// document, calls updated when its subtree is patched and destroyed when it
// leaves. The server owns every value and the option vocabulary; a hook owns
// only transient state (open, query, caret, masked-or-shown). A hook reports
// intent with `this.emit(event, values)`, the runtime's bubbling
// `bilibi:event` CustomEvent, and hears a server push as a `window` event
// `bilibi:<name>` with the payload in detail. ui.js must load first.
(() => {
  // One native input is the keyboard and accessibility contract. Decorative
  // boxes follow its caret, so editing never adds eight tab stops or labels.
  const CodeInput = {
    mounted() {
      this.input = this.el.querySelector("input")
      this.boxes = [...this.el.querySelectorAll("[data-code-index]")]
      this.length = Number(this.el.dataset.codeLength)
      this.normalize = value => value.toUpperCase().replace(/[\s-]/g, "").slice(0, this.length)
      this.paint = () => {
        const active = Math.min(this.input.selectionStart ?? 0, this.length - 1)
        this.boxes.forEach((box, index) => {
          box.textContent = this.input.value[index] || ""
          box.toggleAttribute("data-active", document.activeElement === this.input && index === active)
        })
      }
      this.onInput = () => {
        const caret = this.normalize(this.input.value.slice(0, this.input.selectionStart)).length
        this.input.value = this.normalize(this.input.value)
        this.input.setSelectionRange(caret, caret)
        this.paint()
      }
      this.onPaste = event => {
        const text = event.clipboardData?.getData("text")
        if (text == null) return
        event.preventDefault()
        const start = this.input.selectionStart
        const end = this.input.selectionEnd
        const code = this.normalize(text)
        // A whole code replaces the field even when pasted midway through it.
        this.input.value = code.length === this.length ? code :
          this.normalize(this.input.value.slice(0, start) + code + this.input.value.slice(end))
        const caret = code.length === this.length ? this.length : Math.min(start + code.length, this.length)
        this.input.setSelectionRange(caret, caret)
        this.input.dispatchEvent(new Event("input", {bubbles: true}))
      }
      this.onClick = event => {
        const box = event.target.closest("[data-code-index]")
        if (!box) return
        this.input.focus()
        const index = Math.min(Number(box.dataset.codeIndex), this.input.value.length)
        this.input.setSelectionRange(index, index)
        this.paint()
      }
      this.input.value = this.normalize(this.input.value)
      this.input.classList.add("absolute", "opacity-0", "pointer-events-none")
      this.el.querySelector("[data-code-boxes]").hidden = false
      this.input.addEventListener("input", this.onInput)
      this.input.addEventListener("paste", this.onPaste)
      for (const event of ["focus", "blur", "keyup", "select"]) this.input.addEventListener(event, this.paint)
      this.el.addEventListener("click", this.onClick)
      this.paint()
    },
    updated() {
      this.input.classList.add("absolute", "opacity-0", "pointer-events-none")
      this.el.querySelector("[data-code-boxes]").hidden = false
      this.input.value = this.normalize(this.input.value)
      this.paint()
    },
    destroyed() {
      this.input.removeEventListener("input", this.onInput)
      this.input.removeEventListener("paste", this.onPaste)
      for (const event of ["focus", "blur", "keyup", "select"]) this.input.removeEventListener(event, this.paint)
      this.el.removeEventListener("click", this.onClick)
    },
  }

  // Client-side filtering and keyboard interaction for the combobox.
  //
  // The server owns the hidden form value and the option vocabulary. This hook
  // owns only the transient query, open state and active option, so typing
  // does not cause a round trip or lose the caret: the filter input's own
  // input and change events stop here, before an enclosing form hears them. A
  // choice is committed by copying its value to the hidden input and
  // dispatching a normal bubbling change event; forms therefore keep their
  // existing change and submit contracts. Typing highlights the first match,
  // and Enter while the list is open commits the highlighted option or, with
  // none, does nothing, so it never submits the form.
  const Combobox = {
    mounted() {
      this.input = this.el.querySelector('[role="combobox"]')
      this.valueInput = document.getElementById(this.el.dataset.valueId)
      this.listbox = this.el.querySelector('[role="listbox"]')
      this.noMatches = this.el.querySelector('[id$="-no-matches"]')
      this.clearButton = this.el.querySelector('[id$="-clear"]')
      this.open = false
      this.activeIndex = -1
      this.selectedValue = this.valueInput?.value || ""
      this.selectedLabel = this.input?.value || ""

      this.options = () => Array.from(this.listbox?.querySelectorAll('[role="option"]') || [])

      this.filteredOptions = () => this.options().filter(option => !option.hidden)

      this.setOpen = open => {
        this.open = open
        this.input.setAttribute("aria-expanded", String(open))
        this.listbox.hidden = !open

        if (!open) {
          this.input.removeAttribute("aria-activedescendant")
          this.activeIndex = -1
        }
      }

      this.applyFilter = () => {
        const selectionIsActive = this.input.selectionStart === 0 &&
          this.input.selectionEnd === this.input.value.length
        const query = selectionIsActive ? "" : this.input.value.trim().toLocaleLowerCase()
        const options = this.options()

        options.forEach(option => {
          const label = option.dataset.label.toLocaleLowerCase()
          const value = option.dataset.value.toLocaleLowerCase()
          option.hidden = query !== "" && !label.includes(query) && !value.includes(query)
        })

        const filtered = this.filteredOptions()
        this.noMatches.hidden = options.length === 0 || filtered.length !== 0

        if (this.activeIndex >= filtered.length) this.activeIndex = filtered.length - 1
        this.renderActive()
      }

      this.renderActive = () => {
        const filtered = this.filteredOptions()

        this.options().forEach(option => {
          const active = filtered[this.activeIndex] === option
          option.classList.toggle("bg-brand-surface", active)
          option.setAttribute("aria-selected", String(option.dataset.value === this.selectedValue))
        })

        const active = filtered[this.activeIndex]
        if (active) {
          this.input.setAttribute("aria-activedescendant", active.id)
          active.scrollIntoView({block: "nearest"})
        } else {
          this.input.removeAttribute("aria-activedescendant")
        }
      }

      this.openList = selectQuery => {
        if (!this.open) {
          this.setOpen(true)
          this.activeIndex = -1
          if (selectQuery) this.input.select()
        }

        this.applyFilter()
      }

      this.restore = () => {
        this.selectedValue = this.valueInput?.value || ""
        const selected = this.options().find(option => option.dataset.value === this.selectedValue)
        this.selectedLabel = selected?.dataset.label || ""
        this.input.value = this.selectedLabel
      }

      this.close = () => {
        this.restore()
        this.setOpen(false)
      }

      this.commit = option => {
        if (!option) return

        this.selectedValue = option.dataset.value
        this.selectedLabel = option.dataset.label
        this.input.value = this.selectedLabel
        this.valueInput.value = this.selectedValue
        this.setOpen(false)
        this.valueInput.dispatchEvent(new Event("input", {bubbles: true}))
        this.valueInput.dispatchEvent(new Event("change", {bubbles: true}))
      }

      this.cancel = () => {
        const wasOpen = this.open
        this.close()

        if (!wasOpen || !this.el.dataset.cancelEvent) return

        this.emit(this.el.dataset.cancelEvent)
      }

      this.onInput = event => {
        event.stopPropagation()
        if (!this.open) this.setOpen(true)
        this.activeIndex = 0
        this.applyFilter()
      }
      this.onChange = event => event.stopPropagation()
      this.onFocus = () => this.openList(true)
      this.onKeyDown = event => {
        const filtered = this.filteredOptions()

        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault()
          this.openList(false)
          const delta = event.key === "ArrowDown" ? 1 : -1
          this.activeIndex = filtered.length === 0
            ? -1
            : this.activeIndex < 0
              ? (delta === 1 ? 0 : filtered.length - 1)
              : (this.activeIndex + delta + filtered.length) % filtered.length
          this.renderActive()
        } else if (event.key === "Enter") {
          if (this.open) {
            event.preventDefault()
            this.commit(filtered[this.activeIndex])
          }
        } else if (event.key === "Escape") {
          if (this.open) {
            event.preventDefault()
            this.cancel()
          }
        }
      }

      this.onOptionMouseDown = event => {
        event.preventDefault()
        this.commit(event.currentTarget)
      }

      this.bindOptions = () => {
        this.options().forEach(option => {
          option.removeEventListener("mousedown", this.onOptionMouseDown)
          option.addEventListener("mousedown", this.onOptionMouseDown)
        })
      }

      this.bindClear = () => {
        this.clearButton?.removeEventListener("mousedown", this.onClearMouseDown)
        this.clearButton?.removeEventListener("click", this.onClear)
        this.clearButton = this.el.querySelector('[id$="-clear"]')
        this.clearButton?.addEventListener("mousedown", this.onClearMouseDown)
        this.clearButton?.addEventListener("click", this.onClear)
      }

      this.onDocumentPointerDown = event => {
        if (!this.el.contains(event.target) && this.open) this.cancel()
      }

      this.onFocusOut = event => {
        if (event.relatedTarget && this.el.contains(event.relatedTarget)) return
        requestAnimationFrame(() => {
          if (this.open && !this.el.contains(document.activeElement)) this.cancel()
        })
      }

      this.onClear = event => {
        event.preventDefault()
        this.selectedValue = ""
        this.selectedLabel = ""
        this.input.value = ""
        this.valueInput.value = ""
        this.valueInput.dispatchEvent(new Event("input", {bubbles: true}))
        this.valueInput.dispatchEvent(new Event("change", {bubbles: true}))
        this.input.focus()
        this.openList(false)
      }

      this.onClearMouseDown = event => event.preventDefault()

      this.input.addEventListener("input", this.onInput)
      this.input.addEventListener("change", this.onChange)
      this.input.addEventListener("focus", this.onFocus)
      this.input.addEventListener("keydown", this.onKeyDown)
      this.el.addEventListener("focusout", this.onFocusOut)
      this.bindOptions()
      this.bindClear()
      document.addEventListener("pointerdown", this.onDocumentPointerDown)

      this.applyFilter()
    },

    updated() {
      this.bindOptions()
      this.bindClear()
      if (!this.open) this.restore()
      this.setOpen(this.open)
      this.applyFilter()
    },

    destroyed() {
      this.input.removeEventListener("input", this.onInput)
      this.input.removeEventListener("change", this.onChange)
      this.input.removeEventListener("focus", this.onFocus)
      this.input.removeEventListener("keydown", this.onKeyDown)
      this.el.removeEventListener("focusout", this.onFocusOut)
      this.options().forEach(option => option.removeEventListener("mousedown", this.onOptionMouseDown))
      this.clearButton?.removeEventListener("click", this.onClear)
      this.clearButton?.removeEventListener("mousedown", this.onClearMouseDown)
      document.removeEventListener("pointerdown", this.onDocumentPointerDown)
    },
  }

  // Companion to the reveal option of a password input.
  //
  // The input's `type` is the only record of masked-or-shown: the click flips
  // it, and everything else is derived from it -- the button's accessible
  // name, its title and which glyph shows -- by one read of the attribute, so
  // however many clicks land the state cannot drift, and a stored secret
  // that sets `type` itself (SecretStored) is reflected the same way.
  //
  // A pointer press on the toggle must not pull focus out of the input, and
  // the caret must survive the type swap (browsers reset the selection when
  // `type` changes), so the user can look, then keep typing where they were.
  // Keyboard activation still focuses the button as usual.
  const SecretReveal = {
    mounted() {
      this.input = document.getElementById(this.el.getAttribute("aria-controls"))
      this.showGlyph = document.getElementById(`${this.el.id}-show`)
      this.hideGlyph = document.getElementById(`${this.el.id}-hide`)

      this.sync = () => {
        const shown = this.input.getAttribute("type") === "text"
        const labels = this.el.dataset

        this.el.setAttribute("aria-label", shown ? labels.hideLabel : labels.showLabel)
        this.el.setAttribute("title", shown ? labels.hideTitle : labels.showTitle)
        this.showGlyph.classList.toggle("hidden", shown)
        this.hideGlyph.classList.toggle("hidden", !shown)
      }

      this.onMouseDown = event => event.preventDefault()

      this.onClick = () => {
        const focused = document.activeElement === this.input
        const {selectionStart, selectionEnd} = this.input
        this.input.setAttribute("type", this.input.getAttribute("type") === "text" ? "password" : "text")
        if (focused) this.input.setSelectionRange(selectionStart, selectionEnd)
      }

      this.el.addEventListener("mousedown", this.onMouseDown)
      this.el.addEventListener("click", this.onClick)

      this.masking = new MutationObserver(this.sync)
      this.masking.observe(this.input, {attributeFilter: ["type"]})

      this.sync()
    },

    updated() {
      this.sync()
    },

    destroyed() {
      this.masking.disconnect()
      this.el.removeEventListener("mousedown", this.onMouseDown)
      this.el.removeEventListener("click", this.onClick)
    },
  }

  // Clear a stored secret's keep-current mask in the input property that
  // forms submit. Changing the value attribute would leave the submitted
  // value intact.
  const SecretClear = {
    mounted() {
      this.input = document.getElementById(this.el.dataset.inputId)
      this.clear = () => {
        this.input.value = ""
        this.input.dispatchEvent(new Event("secret:cleared"))
        this.input.focus()
      }
      this.el.addEventListener("click", this.clear)
    },

    destroyed() {
      this.el.removeEventListener("click", this.clear)
    },
  }

  // Only a successful server-side reauthentication sends plaintext to this
  // hook, as a `bilibi:secret:reveal` window event ({id, value, duration_ms})
  // dispatched by the transport. No plaintext
  // is rendered in initial HTML or kept in a hook field. The input property
  // is reset locally even if the connection drops before the timeout. A value
  // the user types or clears during the window is kept, never re-masked.
  const SecretStored = {
    mounted() {
      this.mask = this.el.value
      this.stop = () => {
        clearTimeout(this.timer)
        this.revealing = false
        this.el.type = "password"
      }
      this.onReveal = ({detail}) => {
        const {id, value, duration_ms} = detail || {}
        if (id !== this.el.id) return
        clearTimeout(this.timer)
        this.el.value = value
        this.el.type = "text"
        this.revealing = true
        this.timer = setTimeout(() => this.hide(), duration_ms)
      }
      this.el.addEventListener("secret:cleared", this.stop)
      this.el.addEventListener("input", this.stop)
      window.addEventListener("bilibi:secret:reveal", this.onReveal)
    },

    hide() {
      if (this.revealing) this.el.value = this.mask
      this.stop()
    },

    destroyed() {
      this.hide()
      this.el.removeEventListener("secret:cleared", this.stop)
      this.el.removeEventListener("input", this.stop)
      window.removeEventListener("bilibi:secret:reveal", this.onReveal)
    },
  }

  // Companion to the multi-select and the record history disclosure. The
  // first `[aria-expanded]` inside the wrapper is the trigger whose state is
  // the single record of open; the list and the chevron derive from it in
  // CSS, so they cannot disagree with it. This hook writes that one attribute
  // on every path: the trigger's click toggles it, and a click outside the
  // field, focus leaving the field, and Escape close it.
  //
  // Escape listens on `window`, not the field: Safari and macOS Firefox blur
  // without focusing the button being pressed, so a list opened by mouse
  // there is open while focus sits on `body`, and a listener on the field
  // would never see the key. It is attached only while the list is open, so
  // no closed field is watching keys and no other Escape handler on the page
  // loses one. Reaching that far means the key also arrives from a search
  // box the user is typing in, so only a press with focus already inside the
  // field returns focus to the trigger; everywhere else the list closes and
  // the caret stays where the user put it.
  //
  // A pointer press starting inside the field is not focus leaving it, even
  // when the browser reports no new target: the click that press belongs to
  // already owns the toggle, so dismissing on the blur would close the list
  // and let the same click reopen it. The press ends on the release, which
  // arrives even when the click never does.
  //
  // A wrapper marked `data-keep-on-blur` is a bar a person works in rather
  // than a list they pick from: it closes on its own control and on Escape,
  // and on a click outside it only if it also carries `data-click-away`.
  const DisclosureDismiss = {
    mounted() {
      this.pressing = false
      this.listening = false
      this.trigger = this.el.querySelector("[aria-expanded]")
      this.keepOnBlur = this.el.hasAttribute("data-keep-on-blur")
      this.clickAway = !this.keepOnBlur || this.el.hasAttribute("data-click-away")

      this.isOpen = () => this.trigger.getAttribute("aria-expanded") === "true"
      this.dismiss = () => this.trigger.setAttribute("aria-expanded", "false")
      this.escape = () => {
        this.dismiss()
        this.trigger.focus()
      }
      this.toggle = () => this.trigger.setAttribute("aria-expanded", this.isOpen() ? "false" : "true")

      this.onPointerDown = () => (this.pressing = true)
      this.onPressEnd = () => (this.pressing = false)

      this.onKeyDown = e => {
        if (e.key !== "Escape") return

        if (this.el.contains(document.activeElement)) this.escape()
        else this.dismiss()
      }

      this.onClickAway = e => {
        if (!this.el.contains(e.target)) this.dismiss()
      }

      this.onFocusOut = e => {
        if (this.keepOnBlur) return

        if (this.pressing) {
          this.pressing = false
          return
        }

        if (e.relatedTarget && this.el.contains(e.relatedTarget)) return

        this.dismiss()
      }

      this.syncListeners = () => {
        const open = this.isOpen()
        if (open === this.listening) return

        this.listening = open

        if (open) {
          window.addEventListener("keydown", this.onKeyDown)
          if (this.clickAway) document.addEventListener("click", this.onClickAway)
        } else {
          window.removeEventListener("keydown", this.onKeyDown)
          document.removeEventListener("click", this.onClickAway)
        }
      }

      this.expansion = new MutationObserver(this.syncListeners)
      this.expansion.observe(this.trigger, {attributeFilter: ["aria-expanded"]})

      this.trigger.addEventListener("click", this.toggle)
      this.el.addEventListener("pointerdown", this.onPointerDown)
      this.el.addEventListener("focusout", this.onFocusOut)
      window.addEventListener("pointerup", this.onPressEnd)
      window.addEventListener("pointercancel", this.onPressEnd)

      this.syncListeners()
    },

    destroyed() {
      this.expansion.disconnect()
      this.trigger.removeEventListener("click", this.toggle)
      this.el.removeEventListener("pointerdown", this.onPointerDown)
      this.el.removeEventListener("focusout", this.onFocusOut)
      window.removeEventListener("keydown", this.onKeyDown)
      document.removeEventListener("click", this.onClickAway)
      window.removeEventListener("pointerup", this.onPressEnd)
      window.removeEventListener("pointercancel", this.onPressEnd)
    },
  }

  const registry = window.BilibiUI
  if (!registry) throw new Error("hooks.js needs ui.js loaded first")
  for (const [name, hook] of Object.entries({CodeInput, Combobox, SecretReveal, SecretClear, SecretStored, DisclosureDismiss})) {
    registry.registerHook(name, hook)
  }
})()
