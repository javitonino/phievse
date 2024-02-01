# ϕ-EVSE

An electric vehicle "charger" (EVSE) that can automatically switch between 1 and 3 phase charging.

**This is a personal learning project and it's not production ready. Use it at your own risk.** That said, this might be interesting to learn how this devices work.
Since I was learning myself when I designed this, it should be relatively easy to understand.

## Features

The main differentiator (and motivation for the project) is the ability to switch between 1-phase and 3-phase charging, thus supporting the full range of charging powers.
In the case of 230V AC, this means from 1.4kW (1 phase, 6A) to 22kW (3 phase, 32A).

Charging power can be controlled via web interface or MQTT (integrated with HomeAssistant).

## Anti-features (you really shouldn't connect your car to this)

Although there are safety checks in place (pilot and relay checks), I haven't been able to test them outside of lab conditions. As such, this should not be considered a safe product to use,
which is not ideal (to say the least) for something called Electric Vehicle Safety Equipment.

In general, I lost interest in the project once it behaved well enough to fulfill my needs, so things are not as stable as they should. 

Finally, there are some decisions (documented in the READMEs for [hardware](hardware/README.md) and [firmware](firmware/README.md)) that are not really good ideas and were done in an effort to understand why nobody
else was doing it that way.
