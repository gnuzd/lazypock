import {LazyPock}  from 'lazypock-ts'

const baseUrl = 'http://localhost:8000/api'
export const client = new LazyPock({baseUrl})

