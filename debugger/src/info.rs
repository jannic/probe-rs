use crate::{DebuggerError, DebuggerOptions};

use probe_rs::{
    architecture::{
        arm::{
            ap::{GenericAp, MemoryAp},
            armv6m::Demcr,
            memory::Component,
            sequences::DefaultArmSequence,
            ApAddress, ApInformation, ArmProbeInterface, DpAddress, MemoryApInformation,
        },
        riscv::communication_interface::RiscvCommunicationInterface,
    },
    CoreRegister, DebugProbeError, Probe, ProbeCreationError, WireProtocol,
};

use anyhow::{anyhow, Result};

pub(crate) fn show_info_of_device(debugger_options: &DebuggerOptions) -> Result<(), DebuggerError> {
    let mut probe = match debugger_options.probe_selector.clone() {
        Some(selector) => Probe::open(selector.clone()).map_err(|e| match e {
            DebugProbeError::ProbeCouldNotBeCreated(ProbeCreationError::NotFound) => {
                DebuggerError::Other(anyhow!(
                    "Could not find the probe_selector specified as {:04x}:{:04x}:{:?}",
                    selector.vendor_id,
                    selector.product_id,
                    selector.serial_number
                ))
            }
            other_error => DebuggerError::DebugProbe(other_error),
        }),
        None => {
            // Only automatically select a probe if there is only
            // a single probe detected.
            let list = Probe::list_all();
            if list.len() > 1 {
                return Err(DebuggerError::Other(anyhow!(
                    "Found multiple ({}) probes",
                    list.len()
                )));
            }

            if let Some(info) = list.first() {
                Probe::open(info).map_err(DebuggerError::DebugProbe)
            } else {
                return Err(DebuggerError::Other(anyhow!("No probes found")));
            }
        }
    }?;

    let protocols = if let Some(protocol) = debugger_options.protocol {
        vec![protocol]
    } else {
        vec![WireProtocol::Jtag, WireProtocol::Swd]
    };

    for protocol in protocols {
        let (new_probe, result) = try_show_info(probe, protocol);
        if let Err(e) = result {
            log::warn!(
                "Error identifying target using protocol {}: {}",
                protocol,
                e
            );
        }

        probe = new_probe;

        probe.detach()?;
    }

    Ok(())
}

fn try_show_info(mut probe: Probe, protocol: WireProtocol) -> (Probe, Result<()>) {
    if let Err(e) = probe.select_protocol(protocol) {
        return (probe, Err(e.into()));
    }

    if let Err(e) = probe.attach_to_unspecified() {
        return (probe, Err(e.into()));
    }

    let mut probe = probe;

    if probe.has_arm_interface() {
        match probe.try_into_arm_interface() {
            Ok(interface) => {
                let mut interface = interface
                    .initialize(DefaultArmSequence::new())
                    .expect("This should not be an unwrap");

                if let Err(e) = show_arm_info(&mut interface) {
                    // Log error?
                    log::warn!("Error showing ARM chip information: {}", e);
                }

                probe = interface.close();
            }
            Err((interface_probe, _e)) => {
                probe = interface_probe;
            }
        }
    } else {
        println!(
            "No DAP interface was found on the connected probe. Thus, ARM info cannot be printed."
        );
    }

    if probe.has_riscv_interface() {
        match probe.try_into_riscv_interface() {
            Ok(mut interface) => {
                if let Err(e) = show_riscv_info(&mut interface) {
                    log::warn!("Error showing RISCV chip information: {}", e);
                }

                probe = interface.close();
            }
            Err((interface_probe, _e)) => {
                probe = interface_probe;
            }
        }
    }

    (probe, Ok(()))
}

fn show_arm_info(interface: &mut Box<dyn ArmProbeInterface>) -> Result<()> {
    println!("\nAvailable Access Ports:");

    let dp = DpAddress::Default;
    let num_access_ports = interface.num_access_ports(dp).unwrap();

    for ap_index in 0..num_access_ports {
        let ap = ApAddress {
            ap: ap_index as u8,
            dp,
        };
        let access_port = GenericAp::new(ap);

        let ap_information = interface.ap_information(access_port).unwrap();

        //let idr = interface.read_ap_register(access_port, IDR::default())?;
        //println!("{:#x?}", idr);

        match ap_information {
            ApInformation::MemoryAp(MemoryApInformation {
                debug_base_address, ..
            }) => {
                let access_port: MemoryAp = access_port.into();

                let base_address = *debug_base_address;

                let mut memory = interface.memory_interface(access_port)?;

                // Enable
                // - Data Watchpoint and Trace (DWT)
                // - Instrumentation Trace Macrocell (ITM)
                // - Embedded Trace Macrocell (ETM)
                // - Trace Port Interface Unit (TPIU).
                let mut demcr = Demcr(memory.read_word_32(Demcr::ADDRESS)?);
                demcr.set_dwtena(true);
                memory.write_word_32(Demcr::ADDRESS, demcr.into())?;

                let component_table = Component::try_parse(&mut memory, base_address);

                component_table
                    .iter()
                    .for_each(|entry| println!("{:#08x?}", entry));

                // let mut reader = crate::memory::romtable::RomTableReader::new(&link_ref, baseaddr as u64);

                // for e in reader.entries() {
                //     if let Ok(e) = e {
                //         println!("ROM Table Entry: Component @ 0x{:08x}", e.component_addr());
                //     }
                // }
            }
            ApInformation::Other { .. } => println!("Unknown Type of access port"),
        }
    }

    Ok(())
}

fn show_riscv_info(interface: &mut RiscvCommunicationInterface) -> Result<()> {
    let idcode = interface.read_idcode()?;

    let version = (idcode >> 28) & 0xf;
    let part_number = (idcode >> 12) & 0xffff;
    let manufacturer_id = (idcode >> 1) & 0x7ff;

    let jep_cc = (manufacturer_id >> 7) & 0xf;
    let jep_id = manufacturer_id & 0x3f;

    let jep_id = jep106::JEP106Code::new(jep_cc as u8, jep_id as u8);

    println!("RISCV Chip:");
    println!("\tIDCODE: {:010x}", idcode);
    println!("\t Version:      {}", version);
    println!("\t Part:         {}", part_number);
    println!("\t Manufacturer: {} ({})", manufacturer_id, jep_id);

    Ok(())
}
