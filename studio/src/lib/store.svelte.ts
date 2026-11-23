class AppStore {
  auth = $state<object | null>(null)

  login(email: string, password: string) {
    this.auth = {id: 1}
  }
}

const store = new AppStore()
export default store
