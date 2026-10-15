#pragma once
#include <libremidi/backends/winmm/config.hpp>
#include <libremidi/backends/winmm/helpers.hpp>
#include <libremidi/detail/observer.hpp>

#include <algorithm>
#include <atomic>
#include <optional>
#include <thread>

NAMESPACE_LIBREMIDI
{

class observer_winmm : public observer_api
{
public:
  struct
      : observer_configuration
      , winmm_observer_configuration
  {
  } configuration;

  explicit observer_winmm(observer_configuration&& conf, winmm_observer_configuration&& apiconf)
      : configuration{std::move(conf), std::move(apiconf)}
  {
    if (!configuration.has_callbacks())
      return;

    if (configuration.notify_in_constructor)
      check_new_ports<true>();
    else
      check_new_ports<false>();
  }

  ~observer_winmm() { }

  libremidi::API get_current_api() const noexcept override { return libremidi::API::WINDOWS_MM; }

  std::vector<input_port> get_input_ports() const noexcept override
  {
    return get_port_list<true>();
  }

  std::vector<output_port> get_output_ports() const noexcept override
  {
    return get_port_list<false>();
  }

protected:
  void report_capability_error(const char* direction, std::size_t index, MMRESULT result) const noexcept
  {
    // Polling and explicit enumeration may fail concurrently. Suppress nested
    // error delivery without sharing error_handler's non-atomic recursion flag.
    if (reporting_error.test_and_set(std::memory_order_acquire))
      return;
    error_handler{}.error_impl(
        configuration.on_error,
        "could not read WinMM " + std::string(direction) + " port " + std::to_string(index)
            + " capabilities: " + std::to_string(result),
        source_location::current());
    reporting_error.clear(std::memory_order_release);
  }

  template <bool Notify>
  void check_new_ports()
  {
    auto currInputPortList = get_port_list<true>();

    if constexpr (Notify)
    {
      compare_port_lists_and_notify_clients(
          inputPortList, currInputPortList, configuration.input_added,
          configuration.input_removed);
    }
    inputPortList = std::move(currInputPortList);

    auto currOutputPortList = get_port_list<false>();

    if constexpr (Notify)
    {
      compare_port_lists_and_notify_clients(
          outputPortList, currOutputPortList, configuration.output_added,
          configuration.output_removed);
    }
    outputPortList = std::move(currOutputPortList);
  }

  void compare_port_lists_and_notify_clients(
      const auto& prevList, const auto& currList, const auto& portAddedFunc,
      const auto& portRemovedFunc)
  {
    if (portAddedFunc)
    {
      for (const auto& port : currList)
      {
        auto iter
            = std::ranges::find(prevList, port.display_name, &port_information::display_name);
        if (iter == prevList.end())
          portAddedFunc(port);
      }
    }

    if (portRemovedFunc)
    {
      for (const auto& port : prevList)
      {
        auto iter
            = std::ranges::find(currList, port.display_name, &port_information::display_name);
        if (iter == currList.end())
          portRemovedFunc(port);
      }
    }
  }

  std::optional<input_port> to_in_port_info(std::size_t index) const noexcept
  {
    MIDIINCAPS deviceCaps{};
    const auto result = midiInGetDevCaps(index, &deviceCaps, sizeof(deviceCaps));
    if (result != MMSYSERR_NOERROR)
    {
      report_capability_error("input", index, result);
      return std::nullopt;
    }

    auto rawName = ConvertToUTF8(deviceCaps.szPname);
    return input_port{
        {.api = libremidi::API::WINDOWS_MM,
         .client = 0,
         .device
         = usb_device_identifier{.vendor_id = deviceCaps.wMid, .product_id = deviceCaps.wPid},
         .port = index,
         .manufacturer = "",
         .device_name = "",
         .port_name = rawName,
         .display_name = rawName}};
  }

  std::optional<output_port> to_out_port_info(std::size_t index) const noexcept
  {
    MIDIOUTCAPS deviceCaps{};
    const auto result = midiOutGetDevCaps(index, &deviceCaps, sizeof(deviceCaps));
    if (result != MMSYSERR_NOERROR)
    {
      report_capability_error("output", index, result);
      return std::nullopt;
    }

    auto rawName = ConvertToUTF8(deviceCaps.szPname);
    return output_port{
        {.api = libremidi::API::WINDOWS_MM,
         .client = 0,
         .device
         = usb_device_identifier{.vendor_id = deviceCaps.wMid, .product_id = deviceCaps.wPid},
         .port = index,
         .manufacturer = "",
         .device_name = "",
         .port_name = rawName,
         .display_name = rawName}};
  }

  template <bool Input>
  auto get_port_list() const noexcept
      -> std::vector<std::conditional_t<Input, input_port, output_port>>
  {
    std::vector<std::conditional_t<Input, input_port, output_port>> portList;

    const std::size_t nDevices = Input ? midiInGetNumDevs() : midiOutGetNumDevs();
    for (std::size_t i = 0; i < nDevices; ++i)
    {
      auto port = [&] {
        if constexpr (Input)
          return to_in_port_info(i);
        else
          return to_out_port_info(i);
      }();
      if (port)
      {
        // Use only this enumeration's successful snapshots. Re-querying earlier
        // devices for duplicate names introduces another disappearance race.
        const auto ordinal
            = std::ranges::count(portList, port->port_name, &port_information::port_name) + 1;
        port->display_name += " " + std::to_string(ordinal);
        portList.push_back(std::move(*port));
      }
    }
    return portList;
  }

  static constexpr bool INPUT = true;
  static constexpr bool OUTPUT = false;

  std::vector<input_port> inputPortList;
  std::vector<output_port> outputPortList;

private:
  mutable std::atomic_flag reporting_error = ATOMIC_FLAG_INIT;
};
}

#if __has_include(<stop_token>) && __cpp_lib_jthread >= 201911L
  #include <stop_token>
NAMESPACE_LIBREMIDI::winmm
{
class observer_threaded final : public observer_winmm
{
public:
  explicit observer_threaded(observer_configuration&& conf, winmm_observer_configuration&& apiconf)
      : observer_winmm{std::move(conf), std::move(apiconf)}
  {
    thread = std::jthread([this](std::stop_token tk) {
      while (!tk.stop_requested())
      {
        check_new_ports<true>();
        std::this_thread::sleep_for(this->configuration.poll_period);
      }
    });
  }

private:
  std::jthread thread;
};
}
#else
  #include <atomic>
  #include <semaphore>
NAMESPACE_LIBREMIDI::winmm
{
class observer_threaded final : public observer_winmm
{
public:
  explicit observer_threaded(observer_configuration&& conf, winmm_observer_configuration&& apiconf)
      : observer_winmm{std::move(conf), std::move(apiconf)}
      , sema{0}
  {
    thread = std::thread([this] {
      while (!stop_flag.test(std::memory_order_acquire))
      {
        check_new_ports<true>();
        std::this_thread::sleep_for(this->configuration.poll_period);
      }
      sema.release();
    });
  }

  ~observer_threaded()
  {
    stop_flag.test_and_set();
    sema.acquire();
    thread.join();
  }

private:
  std::thread thread;
  std::atomic_flag stop_flag = ATOMIC_FLAG_INIT;
  std::binary_semaphore sema;
};
}
#endif

NAMESPACE_LIBREMIDI::winmm
{
class observer_manual final : public observer_winmm
{
public:
  explicit observer_manual(observer_configuration&& conf, winmm_observer_configuration&& apiconf)
      : observer_winmm{std::move(conf), std::move(apiconf)}
  {
    this->configuration.manual_poll({.callback = [this] { this->check_new_ports<true>(); }});
  }

  ~observer_manual() { }
};
}
NAMESPACE_LIBREMIDI
{
template <>
inline std::unique_ptr<observer_api> make<observer_winmm>(
    libremidi::observer_configuration&& conf, libremidi::winmm_observer_configuration&& api)
{
  if (api.manual_poll)
    return std::make_unique<winmm::observer_manual>(std::move(conf), std::move(api));
  else
    return std::make_unique<winmm::observer_threaded>(std::move(conf), std::move(api));
}
}
