#include "../include_catch.hpp"
#include <libremidi/detail/callback_lifetime.hpp>

#include <future>

TEST_CASE("Callback self-close is rejected without retiring delivery", "[lifecycle]")
{
  libremidi::callback_lifetime first, second;
  const libremidi::callback_lifetime::scope first_call{first};
  REQUIRE_FALSE(first.begin_close());
  REQUIRE(first_call.accepts_delivery());
  {
    const libremidi::callback_lifetime::scope nested{second};
    REQUIRE_FALSE(first.begin_close());
    REQUIRE_FALSE(second.begin_close());
    REQUIRE(nested.accepts_delivery());
  }
  REQUIRE_FALSE(first.begin_close());
}

TEST_CASE("Retirement retains active callbacks and suppresses later delivery", "[lifecycle]")
{
  libremidi::callback_lifetime callbacks;
  std::promise<void> entered, release;
  auto released = release.get_future();
  std::thread callback{[&] {
    const libremidi::callback_lifetime::scope call{callbacks};
    entered.set_value();
    released.wait();
  }};
  entered.get_future().wait();
  // Release before assertions so a failed assertion cannot leave a joinable
  // thread waiting forever while the test stack unwinds.
  const bool retired = callbacks.begin_close();
  const bool drained_early = callbacks.wait_until(std::chrono::steady_clock::now());
  bool accepted_late{};
  {
    const libremidi::callback_lifetime::scope late{callbacks};
    accepted_late = late.accepts_delivery();
  }
  release.set_value();
  callback.join();
  REQUIRE(retired);
  REQUIRE_FALSE(drained_early);
  REQUIRE_FALSE(accepted_late);
  REQUIRE(callbacks.wait_until(std::chrono::steady_clock::now()));
  REQUIRE(callbacks.begin_close());
  callbacks.resume();
  const libremidi::callback_lifetime::scope reopened{callbacks};
  REQUIRE(reopened.accepts_delivery());
}

TEST_CASE("Self-close stays rejected while another callback is active", "[lifecycle]")
{
  libremidi::callback_lifetime callbacks;
  std::promise<void> entered, release;
  auto released = release.get_future();
  const libremidi::callback_lifetime::scope here{callbacks};
  std::thread other{[&] {
    const libremidi::callback_lifetime::scope there{callbacks};
    entered.set_value();
    released.wait();
  }};
  entered.get_future().wait();
  const bool closed_from_callback = callbacks.begin_close();
  release.set_value();
  other.join();
  REQUIRE_FALSE(closed_from_callback);
  REQUIRE(here.accepts_delivery());
}
