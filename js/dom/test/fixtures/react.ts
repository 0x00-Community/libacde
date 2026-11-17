import * as React from 'react'
import * as ReactDOM from 'react-dom'
import * as ReactDOMClient from 'react-dom/client'

Object.assign(globalThis, { React, ReactDOM, ReactDOMClient, IS_REACT_ACT_ENVIRONMENT: true })
