#pragma once
#include <atomic>
#include <chrono>
#include <thread>

#include <libremidi/config.hpp>

NAMESPACE_LIBREMIDI
{
// Use only while the native callback source still owns a live context. Successful
// source unregistration must precede the final drain and destruction.
class callback_lifetime
{
public:
  class scope
  {
  public:
    explicit scope(callback_lifetime& owner) noexcept : owner{owner}, previous{current}
    {
      owner.active.fetch_add(1);
      current = this;
    }
    ~scope()
    {
      current = previous;
      owner.active.fetch_sub(1);
    }
    scope(const scope&) = delete;
    scope& operator=(const scope&) = delete;

    bool accepts_delivery() const noexcept { return !owner.closing.load(); }

  private:
    friend class callback_lifetime;
    callback_lifetime& owner;
    scope* previous;
    static inline thread_local scope* current{};
  };

  // Reject self-close without changing state. Each thread keeps its own scopes,
  // including nested callbacks; an atomic last-thread ID is not sufficient.
  bool begin_close() noexcept
  {
    for (auto* entry = scope::current; entry; entry = entry->previous)
      if (&entry->owner == this)
        return false;
    closing.store(true);
    return true;
  }

  bool wait_until(std::chrono::steady_clock::time_point deadline) const noexcept
  {
    while (active.load() != 0)
    {
      if (std::chrono::steady_clock::now() >= deadline)
        return false;
      std::this_thread::yield();
    }
    return true;
  }

  // Call only after a successful drain, before registering a new native source.
  void resume() noexcept { closing.store(false); }

private:
  std::atomic_bool closing{false};
  std::atomic_uint active{};
};
}
