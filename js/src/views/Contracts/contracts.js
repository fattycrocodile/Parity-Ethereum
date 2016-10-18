// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

import React, { Component, PropTypes } from 'react';
import { connect } from 'react-redux';
import { bindActionCreators } from 'redux';
import ContentAdd from 'material-ui/svg-icons/content/add';

import { Actionbar, Button, Page } from '../../ui';
import { AddContract, DeployContract } from '../../modals';

import List from '../Accounts/List';

import styles from './contracts.css';

class Contracts extends Component {
  static contextTypes = {
    api: PropTypes.object.isRequired
  }

  static propTypes = {
    balances: PropTypes.object,
    accounts: PropTypes.object,
    contracts: PropTypes.object,
    hasContracts: PropTypes.bool
  }

  state = {
    addContract: false,
    deployContract: false
  }

  render () {
    const { contracts, hasContracts, balances } = this.props;

    return (
      <div className={ styles.contracts }>
        { this.renderActionbar() }
        { this.renderAddContract() }
        { this.renderAddContract() }
        { this.renderDeployContract() }
        <Page>
          <List
            link='contract'
            accounts={ contracts }
            balances={ balances }
            empty={ !hasContracts } />
        </Page>
      </div>
    );
  }

  renderActionbar () {
    const buttons = [
      <Button
        key='addContract'
        icon={ <ContentAdd /> }
        label='watch contract'
        onClick={ this.onAddContract } />,
      <Button
        key='deployContract'
        icon={ <ContentAdd /> }
        label='deploy contract'
        onClick={ this.onDeployContract } />
    ];

    return (
      <Actionbar
        className={ styles.toolbar }
        title='Contracts'
        buttons={ buttons } />
    );
  }

  renderAddContract () {
    const { contracts } = this.props;
    const { addContract } = this.state;

    if (!addContract) {
      return null;
    }

    return (
      <AddContract
        contracts={ contracts }
        onClose={ this.onAddContractClose } />
    );
  }

  renderDeployContract () {
    const { accounts } = this.props;
    const { deployContract } = this.state;

    if (!deployContract) {
      return null;
    }

    return (
      <DeployContract
        accounts={ accounts }
        onClose={ this.onDeployContractClose } />
    );
  }

  onDeployContractClose = () => {
    this.setState({ deployContract: false });
  }

  onDeployContract = () => {
    this.setState({ deployContract: true });
  }

  onAddContractClose = () => {
    this.setState({ addContract: false });
  }

  onAddContract = () => {
    this.setState({ addContract: true });
  }
}

function mapStateToProps (state) {
  const { accounts, contracts, hasContracts } = state.personal;
  const { balances } = state.balances;

  return {
    accounts,
    contracts,
    hasContracts,
    balances
  };
}

function mapDispatchToProps (dispatch) {
  return bindActionCreators({}, dispatch);
}

export default connect(
  mapStateToProps,
  mapDispatchToProps
)(Contracts);
