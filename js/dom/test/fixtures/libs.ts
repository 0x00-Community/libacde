import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { EditorState } from '@codemirror/state'
import { EditorView, keymap, lineNumbers } from '@codemirror/view'
import { Terminal } from '@xterm/xterm'
import DOMPurify from 'dompurify'

Object.assign(globalThis, { CM: { EditorState, EditorView, keymap, lineNumbers, defaultKeymap, history, historyKeymap }, Terminal, DOMPurify })
